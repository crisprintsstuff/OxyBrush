//! Stroke engine and brush kernels.
//!
//! A stroke is a sequence of samples (surface hits from the pointer). The
//! engine spaces them into *dabs*, mirrors each dab for symmetry, and applies
//! the active brush kernel to every vertex inside the dab's sphere. Work is
//! parallelized per chunk; before a chunk is modified the caller's `touch`
//! callback runs so undo can snapshot it.

pub mod alpha;
pub mod detail;
mod falloff;
mod paint_jitter;
pub mod preset;
pub mod recipe;
pub mod wire;

pub use alpha::{Alpha, BuiltinAlpha};
pub use detail::{PartDetail, PartFrame};
pub use falloff::{Falloff, shift_exponent};
pub use paint_jitter::PaintJitter;
pub use preset::BrushPreset;
pub use recipe::{RecipeEvent, StoredChange, StrokeRecipe, SurfaceChange, replay_estimate_ms, resculpt};

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
    /// Paints face groups: each stroke makes a new group (Alt: grows the
    /// group where the stroke starts). Not a dab brush: the app drives it.
    PaintGroups,
    Polish,
    PlanarTrim,
    TopoMove,
    Noise,
    /// A crisp V-shaped crease along the stroke: the surface is pulled
    /// toward the stroke's line from both sides (not toward each dab's
    /// centre, as Crease does), so the walls meet in a sharp fold.
    SharpCrease,
    KelvinGrab,
    KelvinTwist,
    KelvinScale,
    /// Draw a curve, then drag its points: the mesh around it bends and
    /// stretches to follow (see [`wire`]). Not a dab brush: the app drives it.
    CurveMove,
    InsertPrimitive,
    /// Draw a path to get a tube along it, added into the active piece.
    /// Not a dab brush: the app drives it (RFC 0009).
    Tube,
    PlaneClip,
    LineSplit,
    CutAway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InsertShape {
    /// Geodesic (triangles).
    #[default]
    Sphere,
    /// A cube projected onto a sphere: quads, good for subdivision levels.
    QuadSphere,
    /// Rings and segments, poles at top and bottom.
    UvSphere,
    Cube,
    Cylinder,
    Cone,
    Torus,
    Plane,
}

impl InsertShape {
    pub const ALL: [InsertShape; 8] = [
        InsertShape::Sphere,
        InsertShape::QuadSphere,
        InsertShape::UvSphere,
        InsertShape::Cube,
        InsertShape::Cylinder,
        InsertShape::Cone,
        InsertShape::Torus,
        InsertShape::Plane,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InsertShape::Sphere => "Sphere",
            InsertShape::QuadSphere => "Quad sphere",
            InsertShape::UvSphere => "UV sphere",
            InsertShape::Cube => "Cube",
            InsertShape::Cylinder => "Cylinder",
            InsertShape::Cone => "Cone",
            InsertShape::Torus => "Torus",
            InsertShape::Plane => "Plane",
        }
    }

    /// Build a normalized unit mesh for this shape.
    ///
    /// The mesh is positioned such that its base/anchor rests at Y = 0 (Plane at Y = 0),
    /// and its outward orientation points along +Y.
    pub fn build_mesh(self) -> oxy_mesh::Mesh {
        match self {
            InsertShape::Sphere => {
                let mut m = oxy_mesh::primitives::icosphere(3, 1.0);
                for p in &mut m.positions {
                    p.y += 1.0;
                }
                m
            }
            // About as many triangles as the geodesic sphere (1,280).
            InsertShape::QuadSphere => {
                let mut m = oxy_mesh::primitives::quad_sphere(10, 1.0);
                for p in &mut m.positions {
                    p.y += 1.0;
                }
                m
            }
            InsertShape::UvSphere => {
                let mut m = oxy_mesh::primitives::uv_sphere(32, 16, 1.0);
                for p in &mut m.positions {
                    p.y += 1.0;
                }
                m
            }
            InsertShape::Cube => {
                let mut m = oxy_mesh::primitives::cube(4, 2.0);
                for p in &mut m.positions {
                    p.y += 1.0;
                }
                m
            }
            InsertShape::Cylinder => {
                let mut m = oxy_mesh::primitives::cylinder(24, 1.0, 2.0);
                for p in &mut m.positions {
                    p.y += 1.0;
                }
                m
            }
            InsertShape::Cone => {
                let mut m = oxy_mesh::primitives::cone(24, 1.0, 2.0);
                for p in &mut m.positions {
                    p.y += 1.0;
                }
                m
            }
            InsertShape::Torus => {
                let r_major = 1.0;
                let r_minor = 0.35;
                let mut m = oxy_mesh::primitives::torus(24, 16, r_major, r_minor);
                for p in &mut m.positions {
                    p.y += r_minor;
                }
                m
            }
            InsertShape::Plane => oxy_mesh::primitives::plane(8, 2.0),
        }
    }
}

impl BrushKind {
    /// Shelf order; the first ten get number-key shortcuts (1–9, 0).
    pub const ALL: [BrushKind; 28] = [
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
        BrushKind::KelvinGrab,
        BrushKind::KelvinTwist,
        BrushKind::KelvinScale,
        BrushKind::CurveMove,
        BrushKind::Polish,
        BrushKind::PlanarTrim,
        BrushKind::Noise,
        BrushKind::SharpCrease,
        BrushKind::InsertPrimitive,
        BrushKind::Tube,
        BrushKind::PlaneClip,
        BrushKind::LineSplit,
        BrushKind::CutAway,
        BrushKind::Mask,
        BrushKind::Paint,
        BrushKind::PaintGroups,
    ];

    /// Brushes that cut or clamp geometry along a screen-projected plane.
    pub fn is_curve_cut(self) -> bool {
        matches!(self, BrushKind::PlaneClip | BrushKind::LineSplit | BrushKind::CutAway)
    }

    /// Brushes that sculpt with dabs and so can have Gravity: not the
    /// grab-like ones, Mask, Paint, or the tools that place or cut.
    pub fn uses_gravity(self) -> bool {
        !(self.is_grab_like()
            || self.is_curve_cut()
            || matches!(
                self,
                BrushKind::Mask
                    | BrushKind::Paint
                    | BrushKind::PaintGroups
                    | BrushKind::CurveMove
                    | BrushKind::InsertPrimitive
                    | BrushKind::Tube
            ))
    }

    /// Height brushes that offer a per-stroke dab buildup limit.
    pub fn supports_accumulate(self) -> bool {
        matches!(self, Self::Draw | Self::Inflate | Self::Clay | Self::ClayStrips)
    }

    /// Initial buildup setting for a brush in the editor and new presets.
    pub fn default_accumulate(self) -> bool {
        !matches!(self, Self::Draw | Self::Clay | Self::Inflate)
    }

    /// Brushes that move a captured region with the pointer instead of dabbing.
    pub fn is_grab_like(self) -> bool {
        matches!(
            self,
            BrushKind::Grab
                | BrushKind::TopoMove
                | BrushKind::KelvinGrab
                | BrushKind::KelvinTwist
                | BrushKind::KelvinScale
        )
    }

    /// Brushes based on regularized Kelvinlet elasticity solutions.
    pub fn is_kelvin(self) -> bool {
        matches!(self, BrushKind::KelvinGrab | BrushKind::KelvinTwist | BrushKind::KelvinScale)
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
            BrushKind::KelvinGrab => "Kelvin Grab",
            BrushKind::KelvinTwist => "Kelvin Twist",
            BrushKind::KelvinScale => "Kelvin Scale",
            BrushKind::CurveMove => "Curve Move",
            BrushKind::Mask => "Mask",
            BrushKind::Paint => "Paint",
            BrushKind::PaintGroups => "Paint Groups",
            BrushKind::Polish => "Polish",
            BrushKind::PlanarTrim => "Planar Trim",
            BrushKind::TopoMove => "Topological Move",
            BrushKind::Noise => "Noise",
            BrushKind::SharpCrease => "Sharp Crease",
            BrushKind::InsertPrimitive => "Insert Primitive",
            BrushKind::Tube => "Tube",
            BrushKind::PlaneClip => "Plane Clip",
            BrushKind::LineSplit => "Line Split",
            BrushKind::CutAway => "Cut Away",
        }
    }

    /// Default strength that feels balanced for each kernel.
    pub fn default_strength(self) -> f32 {
        match self {
            BrushKind::Smooth => 0.5,
            BrushKind::InsertPrimitive
            | BrushKind::Tube
            | BrushKind::PaintGroups
            | BrushKind::PlaneClip
            | BrushKind::LineSplit
            | BrushKind::CutAway => 1.0,
            BrushKind::Grab
            | BrushKind::SnakeHook
            | BrushKind::TopoMove
            | BrushKind::KelvinGrab
            | BrushKind::KelvinTwist
            | BrushKind::KelvinScale
            | BrushKind::CurveMove
            | BrushKind::Pinch
            | BrushKind::Polish
            | BrushKind::PlanarTrim => 0.5,
            BrushKind::Noise => 0.3,
            BrushKind::Mask => 0.8,
            BrushKind::Paint => 0.6,
            _ => 0.4,
        }
    }
}

/// The direction Gravity pulls: world down (OxyBrush's Y axis is up).
pub const GRAVITY_DOWN: Vec3 = Vec3::NEG_Y;

/// How far Gravity 1 sags the middle of one pass of a stroke, as a share of
/// the brush radius (see `Stroke::sag`): about one radius, so a few passes
/// drape a whole cap into a lip. It was 0.3, which a tester found far too
/// weak next to another app's gravity at half. 2.0 tore the surface at 1.
const GRAVITY_PER_PASS: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushSettings {
    pub kind: BrushKind,
    /// World-space radius of the brush.
    pub radius: f32,
    /// 0..=1
    pub strength: f32,
    pub falloff: Falloff,
    /// Moves the middle of the falloff curve, -1..=1: negative keeps more
    /// of the brush at full strength (firmer), positive less (softer).
    pub focal_shift: f32,
    /// Distance between dabs as a fraction of the radius.
    pub spacing: f32,
    /// Pressure affects strength.
    pub pressure_strength: bool,
    /// Pressure affects radius.
    pub pressure_radius: bool,
    /// Skip vertices facing away from the dab normal (protects thin parts).
    pub front_faces_only: bool,
    /// Allow successive dabs to build on each other within one stroke.
    pub accumulate: bool,
    /// Extra smoothing applied after each dab, 0..=1.
    pub auto_smooth: f32,
    /// Gravity: the surface under the brush also sags downward (world -Y)
    /// as the stroke goes, 0..=1, for draping and hanging folds. At 1 one
    /// full-pressure pass sags the middle of the stroke about one brush
    /// radius (half that at 0.5).
    pub gravity: f32,
    /// Random scatter of dab position, rotation and strength, 0..=1.
    pub spray: f32,
    /// Noise brush: features per brush radius.
    pub noise_scale: f32,
    /// Crease brush: pinch intensity factor (0.0 = no pinch, 0.3 = default, 1.0 = sharp pinch).
    pub crease_pinch: f32,
    /// Insert shape for BrushKind::InsertPrimitive.
    pub insert_shape: InsertShape,
    /// How pen pressure turns into strength and into size (#305): each a
    /// [`shape_pressure`] curve, applied to the sample's pressure.
    pub pressure_curves: [[f32; 4]; 2],
}

/// A straight two-point pressure shape (no change).
pub const LINEAR_PRESSURE_SHAPE: [f32; 4] = [1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0];

/// The cubic through (0,0), (x1,y1), (x2,y2), (1,1), evaluated at `x`.
pub fn shape_pressure(x: f32, shape: [f32; 4]) -> f32 {
    if shape == LINEAR_PRESSURE_SHAPE {
        return x.clamp(0.0, 1.0);
    }
    let [x1, y1, x2, y2] = shape.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 });
    let cubic = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    // x(t) never decreases for control x in 0..1: find t by bisection.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if cubic(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic(0.5 * (lo + hi), y1, y2).clamp(0.0, 1.0)
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            kind: BrushKind::Draw,
            radius: 0.2,
            strength: BrushKind::Draw.default_strength(),
            falloff: Falloff::Smooth,
            focal_shift: 0.0,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            front_faces_only: false,
            accumulate: true,
            auto_smooth: 0.0,
            gravity: 0.0,
            spray: 0.0,
            noise_scale: 3.0,
            crease_pinch: 0.3,
            insert_shape: InsertShape::Sphere,
            pressure_curves: [LINEAR_PRESSURE_SHAPE; 2],
        }
    }
}

/// An alpha applied to a stroke, and how it is oriented on the surface.
#[derive(Debug, Clone, PartialEq)]
pub struct AlphaStamp {
    pub alpha: Arc<Alpha>,
    /// Extra rotation around the dab normal, radians.
    pub rotation: f32,
    /// Align the alpha's x axis with the stroke direction.
    pub follow_stroke: bool,
    /// Invert the alpha: 0 becomes 1 and 1 becomes 0 inside the alpha square.
    pub invert: bool,
    /// World "up" used to orient the alpha when not following the stroke
    /// (typically the camera's up vector, so the alpha appears upright).
    pub up: Vec3,
}

impl AlphaStamp {
    /// Bilinear sample at `uv` in `-1..=1`. 0 outside. If `invert` is true,
    /// inside values are inverted (1 − v).
    pub fn sample(&self, uv: Vec2) -> f32 {
        if uv.x.abs() > 1.0 || uv.y.abs() > 1.0 {
            return 0.0;
        }
        let v = self.alpha.sample(uv);
        if self.invert { (1.0 - v).clamp(0.0, 1.0) } else { v }
    }
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

/// Mirror planes and radial copies. They pass through the stroke's symmetry
/// centre ([`Stroke::with_symmetry_center`]), the world origin by default.
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
    /// True if any mirror plane or radial symmetry copy is active.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.x || self.y || self.z || self.radial > 1
    }

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

/// Where a shape's mirror planes most likely cross, for a piece that has just
/// been imported or opened without its own symmetry centre.
///
/// Each axis is decided on its own. The shape is binned into a coarse
/// occupancy grid, and candidate planes across its bounds are scored by the
/// share of occupied cells whose mirror image is occupied too. The world
/// origin wins whenever it scores about as well as the best plane, so a
/// model built around the origin keeps mirroring there even when it isn't
/// perfectly symmetric (an arm raised on one side, a prop held in one hand);
/// only a shape that is clearly mirrored about somewhere else moves the plane.
pub fn estimate_symmetry_center(positions: &[Vec3]) -> Vec3 {
    const CELLS: f32 = 32.0;
    // Enough points to fill the grid; keeps very dense meshes fast.
    const MAX_SAMPLES: usize = 200_000;
    let finite = || positions.iter().copied().filter(|p| p.is_finite());
    let (lo, hi) = finite().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let extent = (hi - lo).max_element();
    if !(extent.is_finite() && extent > 0.0) {
        return Vec3::ZERO;
    }
    let cell = extent / CELLS;
    let key = |p: Vec3| ((p - lo) / cell).floor().as_ivec3();
    let stride = (positions.len() / MAX_SAMPLES).max(1);
    let occupied: HashSet<glam::IVec3> = finite().step_by(stride).map(key).collect();
    let mut center = Vec3::ZERO;
    for axis in 0..3 {
        // Share of occupied cells whose mirror about `c` is occupied (or one
        // cell away along the axis, which absorbs the grid's rounding).
        let score = |c: f32| -> f32 {
            let hits = occupied
                .iter()
                .filter(|k| {
                    let mut q = lo + (k.as_vec3() + 0.5) * cell;
                    q[axis] = 2.0 * c - q[axis];
                    let m = key(q);
                    let mut step = glam::IVec3::ZERO;
                    step[axis] = 1;
                    occupied.contains(&m) || occupied.contains(&(m + step)) || occupied.contains(&(m - step))
                })
                .count();
            hits as f32 / occupied.len() as f32
        };
        let mid = 0.5 * (lo[axis] + hi[axis]);
        // Half-cell steps across the bounds, nearest the middle first so a
        // tie goes to the most central plane.
        let half_span = 0.5 * (hi[axis] - lo[axis]);
        let steps = (half_span / (0.5 * cell)).ceil() as i32;
        let mut best = (score(mid), mid);
        for k in 1..=steps {
            for c in [mid - k as f32 * 0.5 * cell, mid + k as f32 * 0.5 * cell] {
                let sc = score(c);
                if sc > best.0 + 1e-4 {
                    best = (sc, c);
                }
            }
        }
        let at_origin = if lo[axis] <= 0.0 && hi[axis] >= 0.0 { score(0.0) } else { 0.0 };
        center[axis] = if at_origin >= best.0 - 0.03 { 0.0 } else { best.1 };
    }
    center
}

/// One pointer sample along a stroke.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Gravity: how far a point at full weight also sags, and which way
    /// (zero when off). Added to the brush's own move, so it costs nothing
    /// extra per dab.
    sag: Vec3,
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
    /// This dab copied by a symmetry transform `m` about the point `origin`
    /// (the symmetry centre): positions are mirrored or rotated about it,
    /// directions by `m` alone.
    fn transformed(&self, m: Mat3, origin: Vec3) -> Dab {
        Dab {
            center: origin + m * (self.center - origin),
            normal: m * self.normal,
            direction: m * self.direction,
            drag: m * self.drag,
            tangent: m * self.tangent,
            bitangent: m * self.bitangent,
            view: m * self.view,
            // A mirror copy pulls along its image of down, so mirrored
            // strokes stay mirror images.
            sag: m * self.sag,
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

/// A symmetry copy of an affine Kelvinlet, evaluated from the stroke-start
/// position. A reflected rotation uses an axial vector (det(M) M n).
struct KelvinAffine {
    offset: Vec3,
    axis: Vec3,
    influence: f32,
}

impl KelvinAffine {
    fn displacement(&self, kind: BrushKind, gesture: f32) -> Vec3 {
        let amount = self.influence * gesture;
        if kind == BrushKind::KelvinTwist {
            (glam::Quat::from_axis_angle(self.axis, amount) * self.offset) - self.offset
        } else {
            self.offset * amount.exp_m1()
        }
    }
}

/// One vertex after every symmetry copy that captured it has been merged.
struct BlendedGrab {
    index: u32,
    orig: Vec3,
    /// `blend * delta` is the move for a drag `delta` in the primary sample's
    /// space. The weight is already baked in, so a vertex seen by one copy
    /// moves by `weight * delta`.
    blend: Mat3,
    /// Kelvin Grab's elastic response (bulging beside the drag, pinching
    /// behind it): added as `elastic * delta`, faded out on long drags
    /// ([`elastic_fade`]). Zero for the other grab brushes.
    elastic: Mat3,
    /// Alt (move along the normal): the move is `along * t`.
    along: Vec3,
    affine: Vec<KelvinAffine>,
}

struct GrabState {
    origin: Vec3,
    verts: Vec<BlendedGrab>,
    localized: Vec<(Mat3, Mat3)>,
    /// Average normal of the primary (unmirrored) capture. Alt uses it to
    /// turn the screen drag into a signed distance.
    primary_normal: Vec3,
    /// The brush radius when the grab started (Kelvin Grab's elastic
    /// response fades with the drag measured in it).
    radius: f32,
}

/// Drag length, in brush radii, at which Kelvin Grab's elastic response is
/// down to a half ([`elastic_fade`]). It was 1, which already halved the
/// response on a one-radius pull, so the brush felt like plain Grab. At 3,
/// pulls of up to several radii stay no sharper than plain Grab.
const ELASTIC_FADE_RADII: f32 = 3.0;

/// How much of Kelvin Grab's elastic response to keep for a drag
/// `drag / radius` brush radii long: all of it for a nudge, less and less
/// on a long pull.
///
/// WHY: the response is a linear (small-deformation) formula. On a long
/// pull it pinches the points behind the brush across the drag's centre
/// line, past each other, so the pulled shape folds into a slit along its
/// length (a tester's horns had a crease from tip to base). Fading it keeps
/// the rubbery feel of small moves and clean long pulls.
#[inline]
fn elastic_fade(drag_over_radius: f32) -> f32 {
    let q = drag_over_radius / ELASTIC_FADE_RADII;
    1.0 / (1.0 + q * q)
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

/// Per-vertex state for Layer and the optional buildup limit.
#[derive(Default)]
struct LayerState {
    /// Original position, original normal, and highest weight for Layer.
    vertices: HashMap<u32, (Vec3, Vec3, f32)>,
    buildup: HashMap<u32, f32>,
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
    /// Maximum spacing multiplier to prevent dabs from detaching into separated circles at high resolution.
    pub max_spacing_scale: f32,
    recent: [f32; 8],
    count: usize,
    spacing_scale: f32,
}

impl DabBudget {
    pub fn new(ms: f32) -> Self {
        Self {
            ms,
            max_dabs: 8,
            max_strength_scale: 2.0,
            max_spacing_scale: 1.5,
            recent: [0.0; 8],
            count: 0,
            spacing_scale: 1.0,
        }
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
    paint_jitter: PaintJitter,
    paint_seed: u64,
    rng: u64,
    /// Point that symmetry mirrors and rotates about (world origin unless
    /// set with [`Stroke::set_symmetry_center`]).
    sym_center: Vec3,
    /// Size and strength multipliers for the next samples (see
    /// [`Stroke::set_modulation`]).
    modulation: (f32, f32),
    budget: Option<DabBudget>,
    strength_scale: f32,
    /// How much wider than set the dabs of the current sample are spaced
    /// (1.0 unless a budget, or a replayed recipe, widened them).
    spacing_k: f32,
    symmetry: Symmetry,
    /// The stroke's inputs so far, when it is being recorded
    /// ([`Stroke::with_recording`]).
    log: Option<Vec<RecipeEvent>>,
}

impl Stroke {
    pub fn new(settings: BrushSettings, symmetry: Symmetry, invert: bool) -> Self {
        Self {
            settings,
            symmetry,
            spacing_k: 1.0,
            log: None,
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
            paint_jitter: PaintJitter::default(),
            paint_seed: 0,
            rng: 0x9e37_79b9_7f4a_7c15,
            sym_center: Vec3::ZERO,
            modulation: (1.0, 1.0),
            budget: None,
            strength_scale: 1.0,
        }
    }

    /// Bound each `add_sample` call to approximately `ms` milliseconds by
    /// widening spacing and scaling strength when dabs take longer.
    /// The brush this stroke uses.
    /// Where the stroke is now: its last dab, or its last sample.
    pub fn last_position(&self) -> Option<Vec3> {
        self.last_dab.or(self.last.map(|s| s.position))
    }

    pub fn kind(&self) -> BrushKind {
        self.settings.kind
    }

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
        self.spacing_k
    }

    /// Keep the stroke's inputs as it goes, so [`Stroke::take_recipe`] can
    /// hand back a recipe that makes the same stroke again (RFC 0025).
    pub fn with_recording(mut self) -> Self {
        self.log = Some(Vec::new());
        self
    }

    /// Stop keeping a recipe. Recording stops; [`Stroke::take_recipe`]
    /// then returns None. Used when dynamic detail is on: the triangles
    /// may change under the stroke, so it is excluded whether or not this
    /// stroke happens to rebuild the mesh.
    pub fn discard_recipe(&mut self) {
        self.log = None;
    }

    /// Pretend recent dabs took `ms` milliseconds, so the next samples
    /// plan spacing from that instead of the clock. Tests use it to force
    /// a spacing scale above 1 without depending on how fast the machine is.
    pub fn assume_dab_ms(&mut self, ms: f32) {
        let Some(b) = &mut self.budget else { return };
        let ms = if ms.is_finite() { ms.max(0.0) } else { 0.0 };
        b.recent = [ms; 8];
        b.count = b.recent.len();
    }

    /// The recipe of the stroke so far, if it was recorded and nothing
    /// happened that a recipe can't repeat (the mesh rebuilt under it, or
    /// [`Stroke::discard_recipe`]).
    /// RETURNS: None when not recording; recording stops.
    pub fn take_recipe(&mut self) -> Option<StrokeRecipe> {
        let events = self.log.take()?;
        Some(StrokeRecipe {
            settings: self.settings,
            invert: self.invert,
            symmetry: self.symmetry,
            center: self.sym_center,
            view: self.view,
            grab_through: self.grab_through,
            paint: self.paint,
            paint_jitter: self.paint_jitter,
            paint_seed: self.paint_seed,
            alpha: self.alpha.clone(),
            events,
        })
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

    /// Set the color variation and seed for this Paint stroke.
    pub fn with_paint_jitter(mut self, jitter: PaintJitter, seed: u64) -> Self {
        self.paint_jitter = jitter;
        self.paint_seed = seed;
        self
    }

    pub fn paint_seed(&self) -> u64 {
        self.paint_seed
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

    /// Scale the brush size and strength of the dabs placed by the next
    /// samples (1.0, 1.0 = as set). Used by curve strokes for per-point size
    /// and strength and the fades; it applies whatever the pen-pressure
    /// options are.
    pub fn set_modulation(&mut self, size: f32, strength: f32) {
        self.modulation = (size.max(0.0), strength.max(0.0));
        if let Some(log) = &mut self.log {
            log.push(RecipeEvent::Modulation { size: self.modulation.0, strength: self.modulation.1 });
        }
    }

    /// Mirror and rotate the symmetry copies about `center` instead of the
    /// world origin, so a model that isn't at the origin is still sculpted
    /// symmetrically. Set before the first sample.
    pub fn set_symmetry_center(&mut self, center: Vec3) {
        self.sym_center = center;
    }

    /// Builder form of [`Stroke::set_symmetry_center`].
    pub fn with_symmetry_center(mut self, center: Vec3) -> Self {
        self.sym_center = center;
        self
    }

    pub fn settings(&self) -> &BrushSettings {
        &self.settings
    }

    pub fn dab_count(&self) -> u32 {
        self.dab_count
    }

    /// The mesh was rebuilt mid-stroke (dynamic topology): `source[v]` is
    /// the old index of each new vertex `v` (see
    /// `oxy_mesh::Mesh::source_vertices`), with indices at or past
    /// `old_count` for vertices that didn't exist before. Per-vertex stroke
    /// state follows its vertices; state of removed vertices is dropped.
    pub fn remap_vertices(&mut self, source: &[u32], old_count: usize) {
        // The triangles changed under the stroke, so its inputs alone no
        // longer say what it did. Dynamic detail also drops the recipe
        // when the stroke starts; this catches a rebuild that still
        // reached here.
        self.log = None;
        let mut new_of_old = vec![u32::MAX; old_count];
        for (new, &old) in source.iter().enumerate() {
            if (old as usize) < old_count {
                new_of_old[old as usize] = new as u32;
            }
        }
        let map = |v: u32| new_of_old.get(v as usize).copied().filter(|&n| n != u32::MAX);
        self.layer.vertices =
            std::mem::take(&mut self.layer.vertices).into_iter().filter_map(|(v, s)| Some((map(v)?, s))).collect();
        self.layer.buildup =
            std::mem::take(&mut self.layer.buildup).into_iter().filter_map(|(v, d)| Some((map(v)?, d))).collect();
        if let Some(g) = &mut self.grab {
            g.verts.retain_mut(|b| match map(b.index) {
                Some(n) => {
                    b.index = n;
                    true
                }
                None => false,
            });
        }
    }

    /// Feed a pointer sample. `touch(mesh, chunk)` is called once per chunk
    /// before it is first modified by this call.
    pub fn add_sample(&mut self, mesh: &mut Mesh, sample: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        self.add_sample_planned(mesh, sample, None, touch);
    }

    /// [`Stroke::add_sample`], with the dab plan a recorded stroke used
    /// for this sample (`spacing` scale, `strength` scale, most dabs)
    /// instead of one worked out from how long dabs take now.
    pub(crate) fn add_sample_planned(
        &mut self,
        mesh: &mut Mesh,
        sample: StrokeSample,
        plan: Option<(f32, f32, u32)>,
        touch: &mut dyn FnMut(&Mesh, u32),
    ) {
        if (self.settings.kind.is_grab_like() || self.last.is_none())
            && let Some(log) = &mut self.log
        {
            log.push(RecipeEvent::Sample { sample, spacing: 1.0, strength: self.strength_scale, max_dabs: u32::MAX });
        }
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
        let (k, strength, max_dabs) = match (plan, &mut self.budget) {
            (Some(plan), _) => plan,
            (None, Some(b)) => {
                let wanted = (len + self.carry) / nominal;
                let (n_max, raw_k) = plan_dabs(wanted, b.dab_ms(), b.ms, b.max_dabs);
                let k = raw_k.min(b.max_spacing_scale);
                b.spacing_scale = k;
                (k, k.min(b.max_strength_scale), n_max)
            }
            (None, None) => (1.0, self.strength_scale, u32::MAX),
        };
        self.spacing_k = k;
        self.strength_scale = strength;
        if let Some(log) = &mut self.log {
            log.push(RecipeEvent::Sample { sample, spacing: k, strength, max_dabs });
        }
        let step = nominal * k;
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

    /// A stamp (Stamp stroke mode): one dab at `sample`, applied `repeats`
    /// times in place, so it builds up like the dabs of a short freehand
    /// stroke over the same spot instead of moving the surface the tenth of
    /// a radius a single dab does. Not for the grab-like brushes.
    pub fn stamp(&mut self, mesh: &mut Mesh, sample: StrokeSample, repeats: u32, touch: &mut dyn FnMut(&Mesh, u32)) {
        if self.settings.kind.is_grab_like() {
            return;
        }
        if let Some(log) = &mut self.log {
            log.push(RecipeEvent::Stamp { sample, repeats });
        }
        for _ in 0..repeats.max(1) {
            self.apply_dab(mesh, sample, touch);
        }
        self.last = Some(sample);
        self.carry = 0.0;
    }

    fn make_dab(&self, s: StrokeSample) -> Dab {
        let p = s.pressure.clamp(0.0, 1.0);
        let [strength_curve, size_curve] = self.settings.pressure_curves;
        let radius = if self.settings.pressure_radius {
            self.settings.radius * (0.2 + 0.8 * shape_pressure(p, size_curve))
        } else {
            self.settings.radius
        };
        let strength = if self.settings.pressure_strength {
            self.settings.strength * shape_pressure(p, strength_curve)
        } else {
            self.settings.strength
        };
        let (radius, strength) = (radius * self.modulation.0, strength * self.modulation.1);
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
            sag: self.sag(radius, strength),
        };
        dab.rotated(self.alpha.as_ref().map_or(0.0, |a| a.rotation))
    }

    /// Gravity's sag for a dab of `radius` and (pressure-scaled) `strength`:
    /// toward world down. Scaled by the dab spacing, so it adds up by the
    /// length of the stroke, not by how many dabs land on a point: one pass
    /// at Gravity 1 and full pressure sags the middle of the stroke about one
    /// radius (a point under the middle gets about 2/spacing dabs at an
    /// average weight of about a half; [`GRAVITY_PER_PASS`]). Brush strength doesn't
    /// change it; pen pressure does.
    fn sag(&self, radius: f32, strength: f32) -> Vec3 {
        let g = self.settings.gravity.clamp(0.0, 1.0);
        if g <= 0.0 || !self.settings.kind.uses_gravity() {
            return Vec3::ZERO;
        }
        let pressure = (strength / self.settings.strength.max(1e-6)).clamp(0.0, 1.0);
        let spacing = (self.settings.spacing * self.spacing_scale()).clamp(0.01, 2.0);
        GRAVITY_DOWN * (radius * GRAVITY_PER_PASS * spacing * g * pressure)
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
        let alpha = self.alpha.clone();
        let copies = self.transforms.clone();
        // Where two symmetry copies cover the same vertex, blend their
        // displacements. Writing one after the other lets the second copy
        // chase vertices the first copy already moved, and the overlap tears.
        if copies.len() > 1 && kind == BrushKind::SnakeHook {
            blend_snake_hook(mesh, &self.settings, self.invert, &base, &copies, self.sym_center, alpha.as_ref(), touch);
        } else if copies.len() > 1 && matches!(kind, BrushKind::Pinch | BrushKind::Crease | BrushKind::SharpCrease) {
            blend_symmetry_copies(
                mesh,
                &self.settings,
                self.invert,
                &base,
                &copies,
                self.sym_center,
                alpha.as_ref(),
                touch,
            );
        } else {
            let paint = if kind == BrushKind::Paint && !self.invert && self.paint_jitter.enabled() {
                self.paint_jitter.sample(self.paint, self.paint_seed, self.dab_count - 1)
            } else {
                self.paint
            };
            for &m in &copies {
                let dab = base.transformed(m, self.sym_center);
                apply_kernel(mesh, &self.settings, self.invert, &dab, alpha.as_ref(), paint, &mut self.layer, touch);
                self.auto_smooth_dab(mesh, &dab, touch);
                self.layer_gravity(mesh, &dab, touch);
            }
            return;
        }
        for &m in &copies {
            self.auto_smooth_dab(mesh, &base.transformed(m, self.sym_center), touch);
        }
    }

    /// Gravity for Layer, whose kernel sets heights from the stroke's start
    /// rather than moving points: the sag goes on top, as its own pass. (The
    /// other brushes add it to their own move; see `Dab::sag`.)
    fn layer_gravity(&mut self, mesh: &mut Mesh, dab: &Dab, touch: &mut dyn FnMut(&Mesh, u32)) {
        if self.settings.kind != BrushKind::Layer || dab.sag == Vec3::ZERO {
            return;
        }
        // Brush-shaped weights, without the alpha.
        let shape = BrushSettings { kind: BrushKind::Draw, ..self.settings };
        let (chunks, affected) = gather_affected(mesh, &shape, dab, None);
        if affected.is_empty() {
            return;
        }
        for &c in &chunks {
            touch(mesh, c);
        }
        let mut moved = Vec::with_capacity(affected.len());
        for &(v, w) in &affected {
            mesh.positions[v as usize] += dab.sag * w;
            moved.push(v);
        }
        mesh.update_after_move(&moved);
        // Later dabs set Layer's heights from where each point started:
        // move that start too, so the sag adds up along the stroke.
        for &(v, w) in &affected {
            if let Some(e) = self.layer.vertices.get_mut(&v) {
                e.0 += dab.sag * w;
            }
        }
    }

    fn auto_smooth_dab(&mut self, mesh: &mut Mesh, dab: &Dab, touch: &mut dyn FnMut(&Mesh, u32)) {
        let kind = self.settings.kind;
        if self.settings.auto_smooth > 0.0 && !matches!(kind, BrushKind::Smooth | BrushKind::Mask | BrushKind::Paint) {
            // The smoothing pass doesn't sag again (`sag` is the brush's).
            let smooth = Dab { strength: self.settings.auto_smooth, sag: Vec3::ZERO, ..*dab };
            let smooth_settings = BrushSettings { kind: BrushKind::Smooth, ..self.settings };
            apply_kernel(mesh, &smooth_settings, false, &smooth, None, self.paint, &mut self.layer, touch);
        }
    }

    fn grab_sample(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        self.grab_sample_with_kernel(mesh, s, touch, kelvin_grab_tensor, true);
    }

    fn grab_sample_with_kernel(
        &mut self,
        mesh: &mut Mesh,
        s: StrokeSample,
        touch: &mut dyn FnMut(&Mesh, u32),
        kernel: impl Fn(Vec3, f32) -> (Mat3, Mat3),
        tuned: bool,
    ) {
        let Some(grab) = &self.grab else {
            // First sample: capture the vertices under each transformed brush.
            let dab = self.make_dab(StrokeSample { pressure: 1.0, ..s });
            let alpha = self.alpha.clone();
            let kind = self.settings.kind;
            let affine = matches!(kind, BrushKind::KelvinTwist | BrushKind::KelvinScale);
            let tune_grab = tuned && kind == BrushKind::KelvinGrab;
            let topological = self.settings.kind == BrushKind::TopoMove;
            let through = self.grab_through && self.view != Vec3::ZERO && !topological;
            let captured: Vec<(Mat3, Vec3, Vec<GrabHit>)> = self
                .transforms
                .iter()
                .map(|&m| {
                    let d = dab.transformed(m, self.sym_center);
                    let mut verts = Vec::new();
                    if through {
                        verts = capture_cylinder(mesh, &self.settings, &d);
                    } else {
                        for c in mesh.chunks_in_sphere(d.center, reach(self.settings.kind, &d, alpha.is_some())) {
                            for v in mesh.chunks[c as usize].vertices.clone() {
                                if let Some(w) = vertex_weight(mesh, &self.settings, &d, alpha.as_ref(), v) {
                                    verts.push((v, w, mesh.positions[v as usize]));
                                }
                            }
                        }
                    }
                    if topological {
                        verts = connected_to_nearest(mesh, d.center, verts);
                    }
                    (m, d.center, verts)
                })
                .collect();
            let normals: Vec<Vec3> = captured
                .iter()
                .zip(&self.transforms)
                .map(|((_, _, verts), &m)| {
                    let n: Vec3 = verts.iter().map(|&(v, w, _)| mesh.normals[v as usize] * w).sum();
                    n.try_normalize().unwrap_or(m * dab.normal)
                })
                .collect();
            // Combine symmetry copies additively: each copy that reaches a
            // vertex contributes its own move times its falloff weight (Grab
            // and Topo Move: its mirrored drag; Kelvin brushes: their own
            // tensor), and the contributions are summed. Where the mirrored
            // brushes overlap near the plane they reinforce each other, so
            // the shape rises in one piece instead of leaving a valley or a
            // ridge. The part of the sum that counts the same brush twice is
            // divided out ([`overlap_divisor`]), so the point under the
            // cursor follows it exactly and the overlap stays round.
            // Opposite components (the two sides' drags across the plane)
            // cancel smoothly, keeping the seam on the plane.
            struct Acc {
                orig: Vec3,
                sum_m: Mat3,
                sum_e: Mat3,
                sum_n: Vec3,
                sum_w: f32,
                max_w: f32,
                affine: Vec<KelvinAffine>,
            }
            let centers: Vec<Vec3> = captured.iter().map(|c| c.1).collect();
            let overlap = copies_overlap(
                &self.settings,
                dab.radius,
                &centers,
                alpha.is_some(),
                through.then_some(self.view.normalize_or_zero()),
                &kernel,
            );
            let local_overlap = if tune_grab {
                copies_overlap(
                    &self.settings,
                    dab.radius,
                    &centers,
                    alpha.is_some(),
                    through.then_some(self.view.normalize_or_zero()),
                    biscale_grab_tensor,
                )
            } else {
                0.0
            };
            let mut local = HashMap::<u32, (Mat3, Mat3)>::new();
            let mut acc: HashMap<u32, Acc> = HashMap::new();
            for ((m, center, hits), n) in captured.iter().zip(&normals) {
                for &(index, w, orig) in hits {
                    let e = acc.entry(index).or_insert(Acc {
                        orig,
                        sum_m: Mat3::ZERO,
                        sum_e: Mat3::ZERO,
                        sum_n: Vec3::ZERO,
                        sum_w: 0.0,
                        max_w: 0.0,
                        affine: Vec::new(),
                    });
                    let offset = orig - *center;
                    if affine {
                        // de Goes and James (2017), equations 15 and 16.
                        // Normalize the affine field to unit gradient at contact.
                        let inverse_r2 = 1.0 / (1.0 + (offset / dab.radius.max(1e-6)).length_squared());
                        let kernel = inverse_r2 * inverse_r2.sqrt() * (0.4 + 0.6 * inverse_r2);
                        let influence = w * self.settings.strength * kernel;
                        e.affine.push(KelvinAffine { offset, axis: *n * m.determinant().signum(), influence });
                    } else {
                        let weight = self.settings.strength * w;
                        if kind == BrushKind::KelvinGrab {
                            let (carry, elastic) = kernel(offset, dab.radius);
                            e.sum_m += carry * *m * weight;
                            e.sum_e += elastic * *m * weight;
                            if tune_grab {
                                let (carry, elastic) = biscale_grab_tensor(offset, dab.radius);
                                let sum = local.entry(index).or_insert((Mat3::ZERO, Mat3::ZERO));
                                sum.0 += carry * *m * weight;
                                sum.1 += elastic * *m * weight;
                            }
                        } else {
                            e.sum_m += *m * weight;
                            e.sum_n += *n * weight;
                        }
                    }
                    e.sum_w += w;
                    e.max_w = e.max_w.max(w);
                }
            }
            let mut localized = Vec::with_capacity(local.len());
            let verts = acc
                .into_iter()
                .map(|(index, mut e)| {
                    // One copy: scale 1, so the move is exactly that copy's.
                    let scale = if e.sum_w > 1e-8 { 1.0 / overlap_divisor(e.sum_w, e.max_w, overlap) } else { 0.0 };
                    if tune_grab {
                        let (carry, elastic) = local[&index];
                        let scale = 1.0 / overlap_divisor(e.sum_w, e.max_w, local_overlap);
                        localized.push((carry * scale, elastic * scale));
                    }
                    for affine in &mut e.affine {
                        affine.influence *= scale;
                    }
                    BlendedGrab {
                        index,
                        orig: e.orig,
                        blend: e.sum_m * scale,
                        elastic: e.sum_e * scale,
                        along: e.sum_n * scale,
                        affine: e.affine,
                    }
                })
                .collect();
            let primary_normal = normals.first().copied().unwrap_or(dab.normal);
            self.grab = Some(GrabState { origin: s.position, verts, localized, primary_normal, radius: dab.radius });
            self.last = Some(s);
            return;
        };
        let delta = s.position - grab.origin;
        let kind = self.settings.kind;
        let affine = matches!(kind, BrushKind::KelvinTwist | BrushKind::KelvinScale);
        // Alt constrains translation to the normal and reverses affine gestures.
        let normal_drag = (self.invert && !affine).then(|| along_normal(delta, grab.primary_normal, self.view));
        let gesture = if affine {
            let view = self.view.normalize_or(-Vec3::Z);
            let right = view.cross(Vec3::Y).normalize_or(Vec3::X);
            let up = right.cross(view);
            let axis = if kind == BrushKind::KelvinTwist { right } else { up };
            let sign = if self.invert { -1.0 } else { 1.0 };
            (delta.dot(axis) / grab.radius.max(1e-6) * sign).clamp(-2.0, 2.0)
        } else {
            0.0
        };
        let mut chunks: Vec<u32> = grab.verts.iter().map(|v| mesh.vertex_chunk(v.index)).collect();
        chunks.sort_unstable();
        chunks.dedup();
        for c in chunks {
            touch(mesh, c);
        }
        let drag_in_radii = delta.length() / grab.radius.max(1e-6);
        let fade = elastic_fade(drag_in_radii);
        let mix = biscale_mix(drag_in_radii);
        let mut moved = Vec::with_capacity(grab.verts.len());
        for (i, v) in grab.verts.iter().enumerate() {
            let d = if affine {
                v.affine.iter().map(|copy| copy.displacement(kind, gesture)).sum()
            } else if let Some(normal_drag) = normal_drag.filter(|_| kind != BrushKind::KelvinGrab) {
                v.along * normal_drag.dot(grab.primary_normal)
            } else {
                let drag = normal_drag.unwrap_or(delta);
                let elastic_drag = drag * fade;
                let broad = v.blend * drag + v.elastic * elastic_drag;
                if mix > 0.0
                    && let Some((carry, elastic)) = grab.localized.get(i)
                {
                    broad.lerp(*carry * drag + *elastic * elastic_drag, mix)
                } else {
                    broad
                }
            };
            mesh.positions[v.index as usize] = v.orig + d;
            moved.push(v.index);
        }
        mesh.update_after_move(&moved);
        self.dab_count += 1;
        self.last = Some(s);
    }
}

/// Translational state of the regularized Kelvinlet (de Goes and James,
/// "Regularized Kelvinlets", SIGGRAPH 2017, DOI 10.1145/3072959.3073595).
/// Normalize at the contact and measure offsets in units of epsilon. Keep
/// the isotropic and anisotropic terms separate for the long-drag limiter.
fn kelvin_grab_tensor(offset: Vec3, radius: f32) -> (Mat3, Mat3) {
    let r = offset / radius.max(1e-6);
    let s = (1.0 + r.length_squared()).sqrt();
    let s3 = s * s * s;
    let poisson = 0.45;
    let b = 1.0 / (4.0 * (1.0 - poisson));
    let norm = 1.0 / (1.5 - b);
    let outer = Mat3::from_cols(r * r.x, r * r.y, r * r.z);
    (Mat3::IDENTITY * (norm * ((1.0 - b) / s + 0.5 / s3)), outer * (norm * b / s3))
}

/// Equation 9 from de Goes and James (2017). The single-scale tensors
/// are already normalized at contact, so divide each by its radius to
/// recover the relative kernel amplitudes before subtracting them.
fn biscale_grab_tensor(offset: Vec3, radius: f32) -> (Mat3, Mat3) {
    let scale = 1.1;
    let (core, elastic_core) = kelvin_grab_tensor(offset, radius);
    let (wide, elastic_wide) = kelvin_grab_tensor(offset, radius * scale);
    ((core * scale - wide) / (scale - 1.0), (elastic_core * scale - elastic_wide) / (scale - 1.0))
}

/// Use the localized field on nudges, and recover the broader support as
/// the drag grows to avoid concentrating strain at a horn's root.
fn biscale_mix(drag_in_radii: f32) -> f32 {
    let t = (drag_in_radii - 0.5).clamp(0.0, 1.0);
    0.55 * (1.0 - t * t * (3.0 - 2.0 * t))
}

const KELVIN_REACH: f32 = 2.0;

/// A brush's falloff at squared distance `d2` from its centre, without the
/// alpha image, the mask or front-face fading: the shape `vertex_weight`
/// gives a plain brush. `alpha` widens the footprint as an alpha does.
#[inline]
fn falloff_profile(settings: &BrushSettings, radius: f32, d2: f32, alpha: bool) -> f32 {
    if settings.kind.is_kelvin() {
        // Smooth finite support for interactive chunk capture. The Kelvinlet
        // itself supplies the core falloff, including the response outside
        // the drawn brush radius.
        let t = (d2.sqrt() / (radius * KELVIN_REACH).max(1e-6)).min(1.0);
        let edge = 1.0 - t * t;
        edge * edge
    } else if alpha {
        // With an alpha the image defines the footprint; falloff still softens it.
        settings.falloff.eval_shifted((d2.sqrt() / (radius * std::f32::consts::SQRT_2)).min(1.0), settings.focal_shift)
    } else {
        settings.falloff.eval_shifted(d2.sqrt() / radius, settings.focal_shift)
    }
}

/// How much the symmetry copies of one dab overlap: the other copies'
/// falloff weights at the first copy's centre, summed (0 when they don't
/// reach each other; 1 for two copies on top of each other). The copies are
/// images of one another, so every copy sees the same sum. `view`, for a
/// grab-through capture, measures distances across the view only.
fn copies_overlap(
    settings: &BrushSettings,
    radius: f32,
    centers: &[Vec3],
    alpha: bool,
    view: Option<Vec3>,
    kernel: impl Fn(Vec3, f32) -> (Mat3, Mat3),
) -> f32 {
    let Some(&c0) = centers.first() else { return 0.0 };
    centers[1..]
        .iter()
        .map(|&c| {
            let mut off = c - c0;
            if let Some(v) = view {
                off -= v * off.dot(v);
            }
            let weight = falloff_profile(settings, radius, off.length_squared(), alpha).max(0.0);
            if settings.kind == BrushKind::KelvinGrab {
                // The capture taper only bounds work. Use the translational
                // kernel's carried response to judge overlap at the contact.
                weight * kernel(off, radius).0.x_axis.x.max(0.0)
            } else {
                weight
            }
        })
        .sum()
}

/// What the summed contributions of a grab-like brush's symmetry copies are
/// divided by at a vertex, from the sum `sum_w` of the copies' falloff
/// weights there, the strongest `max_w`, and how much the copies overlap
/// each other (`overlap`, [`copies_overlap`]).
///
/// WHY: each copy moves a vertex by its own drag times its weight, and the
/// copies add up, so where mirrored brushes overlap near the plane the shape
/// rises in one piece (taking only the strongest copy left a valley there).
/// Added up as they are, two copies on top of each other would move a point
/// twice; 4.0.0-beta.21 capped the sum at one drag, which flattened every
/// overlap into a mesa ("the points are capped, so it isn't round").
/// Instead, the part of the sum that is the copies counting the same brush
/// twice is divided out: with `r` the mirror copies' share at this vertex
/// relative to their share under the cursor, the sum is divided by
/// `1 + overlap * r`. So:
///
/// - the vertex under the cursor follows the drag exactly (`r` = 1 there);
/// - copies on top of each other (brush on the plane) are exactly one brush;
/// - where only one copy reaches, the move is exactly that copy's (`r` = 0),
///   so strokes away from the plane are unchanged;
/// - copies that don't reach each other (`overlap` 0) simply add up;
/// - in between the shape stays round: no cap, no crease. Between two copies
///   a little off the plane the seam can rise a little past the drag (up to
///   about 17% at 0.3–0.4 radii from the plane), as two overlapping pulls do.
///
/// `r` eases in (`1 - (1 - g)²`), so the divisor has no kink where the
/// mirror copy's share reaches its share under the cursor.
///
/// RETURNS: the divisor, at least 1.
fn overlap_divisor(sum_w: f32, max_w: f32, overlap: f32) -> f32 {
    if overlap <= 1e-6 || max_w <= 1e-8 {
        return 1.0;
    }
    let others = (sum_w - max_w).max(0.0);
    let g = (others / (max_w * overlap)).min(1.0);
    let r = 1.0 - (1.0 - g) * (1.0 - g);
    1.0 + overlap * r
}

/// Grab-through capture: every visible vertex whose distance from the
/// line through the dab centre along the view direction is within the
/// radius, front to back, with the brush falloff and the mask applied.
fn capture_cylinder(mesh: &Mesh, settings: &BrushSettings, dab: &Dab) -> Vec<GrabHit> {
    let r = reach(settings.kind, dab, false);
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
            let w = falloff_profile(settings, dab.radius, radial * radial, false) * (1.0 - mesh.mask[v as usize]);
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
        for &n in &mesh.neighbors(v) {
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
    if kind.is_kelvin() && !has_alpha {
        dab.radius * KELVIN_REACH
    } else if kind == BrushKind::ClayStrips || has_alpha {
        dab.radius * std::f32::consts::SQRT_2
    } else {
        dab.radius
    }
}

/// Falloff weight × alpha × (1 − mask) for a vertex, or None if outside the brush.
fn vertex_weight(mesh: &Mesh, settings: &BrushSettings, dab: &Dab, alpha: Option<&AlphaStamp>, v: u32) -> Option<f32> {
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
    let facing = if settings.front_faces_only {
        let c = mesh.normals[v as usize].dot(dab.normal);
        if c <= 0.0 {
            return None;
        }
        front_fade(c)
    } else {
        1.0
    };
    let f = if settings.kind == BrushKind::ClayStrips && dab.direction != Vec3::ZERO {
        // Square footprint aligned with the stroke: soft only across it.
        let side = dab.normal.cross(dab.direction);
        let along = offset.dot(dab.direction).abs() / dab.radius;
        let across = offset.dot(side).abs() / dab.radius;
        if along > 1.0 || across > 1.0 {
            0.0
        } else {
            settings.falloff.eval_shifted(across, settings.focal_shift).max(0.0) * (1.0 - along.powi(8))
        }
    } else {
        falloff_profile(settings, dab.radius, d2, alpha.is_some())
    };
    let f = match alpha {
        Some(a) => f * a.sample(Vec2::new(offset.dot(dab.tangent), offset.dot(dab.bitangent)) / dab.radius),
        None => f,
    };
    let mask = if settings.kind == BrushKind::Mask { 0.0 } else { mesh.mask[v as usize] };
    let w = f * facing * (1.0 - mask);
    (w > 0.0).then_some(w)
}

/// Below this cosine between a vertex normal and the brush normal, "front
/// faces only" starts fading the brush out (about 73° from facing the brush).
const FRONT_FADE: f32 = 0.3;

/// How much of the brush a vertex gets under "front faces only", from the
/// cosine `c` between its normal and the brush normal: full when it faces
/// the brush, easing to zero as it turns sideways, nothing facing away.
///
/// INVARIANT: continuous in `c`. A hard cut at 90° moved a vertex fully
/// while its neighbour just past the cut stayed put, which tore steep walls
/// (the sides of a pulled-out bulge, the valley between mirrored bulges)
/// into jagged seams, worst with Grab.
#[inline]
fn front_fade(c: f32) -> f32 {
    if c >= FRONT_FADE {
        return 1.0;
    }
    let t = (c / FRONT_FADE).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Chunks in range of a dab, and the weighted vertices inside them.
/// Snake Hook queries where the brush was before this dab's drag.
fn gather_affected(
    mesh: &Mesh,
    settings: &BrushSettings,
    dab: &Dab,
    alpha: Option<&AlphaStamp>,
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
            let pinch_factor = if kind == BrushKind::Crease { settings.crease_pinch } else { 0.3 };
            let pinch = |p: Vec3| {
                let to_center = dab.center - p;
                (to_center - area_normal * to_center.dot(area_normal)) * (pinch_factor * w * dab.strength)
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
                // Sharp Crease: pull each point across the stroke toward
                // its line (never past it), and cut deepest on the line,
                // so the two walls meet in a V. Before the stroke has a
                // direction (the first dab) it gathers toward the centre
                // like Crease. Inverted it raises a sharp ridge.
                BrushKind::SharpCrease => {
                    // The groove behind the brush tilts the mean normal
                    // forward or back; without its part along the stroke
                    // the cut stays straight down and the crease straight.
                    let normal = (area_normal - dab.direction * area_normal.dot(dab.direction))
                        .try_normalize()
                        .unwrap_or(area_normal);
                    let side = normal.cross(dab.direction).normalize_or_zero();
                    let offset = p - dab.center;
                    let (across, gather) = if side == Vec3::ZERO {
                        let flat = offset - normal * offset.dot(normal);
                        (flat.length(), -flat)
                    } else {
                        let c = offset.dot(side);
                        (c.abs(), -side * c)
                    };
                    // Never more than 90% of the way to the line per dab,
                    // so points don't cross it and fold the surface over.
                    let k = (settings.crease_pinch.max(0.0) * w * dab.strength).min(0.9);
                    let v = (1.0 - across / dab.radius).max(0.0);
                    p + gather * k - normal * (sign * amount * 0.8 * v * v * w)
                }
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
                BrushKind::Grab
                | BrushKind::TopoMove
                | BrushKind::KelvinGrab
                | BrushKind::KelvinTwist
                | BrushKind::KelvinScale
                | BrushKind::CurveMove
                | BrushKind::Mask
                | BrushKind::Paint
                | BrushKind::PaintGroups
                | BrushKind::Layer
                | BrushKind::InsertPrimitive
                | BrushKind::Tube
                | BrushKind::PlaneClip
                | BrushKind::LineSplit
                | BrushKind::CutAway => p,
            };
            (v, np + dab.sag * w)
        })
        .collect()
}

/// Snake Hook with more than one symmetry copy. Each copy's drag is read
/// from the pre-dab surface and blended like Grab: the copies add up where
/// they overlap, less the part that counts the same brush twice
/// ([`overlap_divisor`]), so the seam rises with the brush in one round
/// piece instead of lagging into a valley or flattening into a mesa.
#[allow(clippy::too_many_arguments)]
fn blend_snake_hook(
    mesh: &mut Mesh,
    settings: &BrushSettings,
    invert: bool,
    base: &Dab,
    copies: &[Mat3],
    origin: Vec3,
    alpha: Option<&AlphaStamp>,
    touch: &mut dyn FnMut(&Mesh, u32),
) {
    let mut acc: HashMap<u32, (Vec3, f32, f32)> = HashMap::new();
    let mut chunks = Vec::new();
    let centers: Vec<Vec3> = copies.iter().map(|&m| base.transformed(m, origin).center).collect();
    let overlap = copies_overlap(settings, base.radius, &centers, alpha.is_some(), None, kelvin_grab_tensor);
    for &m in copies {
        let dab = base.transformed(m, origin);
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
            e.0 += drag * (scale * w) + dab.sag * w;
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
        mesh.positions[*v as usize] += *weighted / overlap_divisor(*sum_w, *max_w, overlap);
    }
    mesh.update_after_move(&moved);
}

/// Pinch and Crease slide vertices toward each copy. Copies are read from
/// the pre-dab surface and blended like a drag, so the overlap follows the
/// stronger copy instead of being pulled both ways at once.
///
/// Crease and Sharp Crease blend only the slide across the surface. The
/// groove along the normal is summed instead: blending it shrinks a vertex
/// on the mirror plane (both copies equally strong) to one copy's depth
/// while its neighbours, reached by different dabs over the stroke, keep
/// more, a ridge along the plane. A sum is smooth across it.
#[allow(clippy::too_many_arguments)]
fn blend_symmetry_copies(
    mesh: &mut Mesh,
    settings: &BrushSettings,
    invert: bool,
    base: &Dab,
    copies: &[Mat3],
    origin: Vec3,
    alpha: Option<&AlphaStamp>,
    touch: &mut dyn FnMut(&Mesh, u32),
) {
    let grooves = settings.kind != BrushKind::Pinch;
    // (weighted slide, sum of weights, strongest weight, summed groove)
    let mut acc: HashMap<u32, (Vec3, f32, f32, Vec3)> = HashMap::new();
    let mut chunks = Vec::new();
    for &m in copies {
        let dab = base.transformed(m, origin);
        let (ch, affected) = gather_affected(mesh, settings, &dab, alpha);
        if affected.is_empty() {
            continue;
        }
        chunks.extend(ch);
        let area_normal = area_normal_of(mesh, &affected, dab.normal);
        let normal = if settings.kind == BrushKind::SharpCrease {
            (area_normal - dab.direction * area_normal.dot(dab.direction)).try_normalize().unwrap_or(area_normal)
        } else {
            area_normal
        };
        let weight_of: HashMap<u32, f32> = affected.iter().copied().collect();
        for (v, p) in target_positions(mesh, settings, invert, &dab, &affected) {
            let w = weight_of[&v];
            let delta = p - mesh.positions[v as usize];
            let groove = if grooves { normal * (delta - dab.sag * w).dot(normal) } else { Vec3::ZERO };
            let e = acc.entry(v).or_insert((Vec3::ZERO, 0.0, 0.0, Vec3::ZERO));
            e.0 += delta - groove;
            e.1 += w;
            e.2 = e.2.max(w);
            e.3 += groove;
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
    for (v, (weighted, sum_w, max_w, groove)) in &acc {
        mesh.positions[*v as usize] += blend_weighted_deltas(*weighted, *sum_w, *max_w) + *groove;
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
    alpha: Option<&AlphaStamp>,
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

    let capped = !settings.accumulate && kind.supports_accumulate();
    let without_sag;
    let kernel_dab = if capped {
        without_sag = Dab { sag: Vec3::ZERO, ..*dab };
        &without_sag
    } else {
        dab
    };
    let new_positions = target_positions(mesh, &settings, invert, kernel_dab, &affected);

    let mut moved = Vec::with_capacity(new_positions.len());
    if capped {
        for ((v, candidate), (_, weight)) in new_positions.into_iter().zip(affected) {
            let position = &mut mesh.positions[v as usize];
            let delta = candidate - *position;
            let length = delta.length();
            let maximum = layer.buildup.entry(v).or_default();
            let mut step = dab.sag * weight;
            if length > *maximum {
                step += delta * ((length - *maximum) / length);
                *maximum = length;
            }
            if step != Vec3::ZERO {
                *position += step;
                moved.push(v);
            }
        }
    } else {
        for (v, p) in new_positions {
            mesh.positions[v as usize] = p;
            moved.push(v);
        }
    }
    mesh.update_after_move(&moved);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_limits_overlap_and_resets_between_strokes() {
        let settings = BrushSettings {
            radius: 0.25,
            strength: 0.8,
            falloff: Falloff::Constant,
            accumulate: false,
            ..Default::default()
        };
        for invert in [false, true] {
            let base = oxy_mesh::primitives::icosphere(4, 1.0);
            let center = base.positions.iter().position(|p| *p == Vec3::Y).unwrap();
            let mut mesh = base.clone();
            let mut stroke = Stroke::new(settings, Symmetry::default(), invert);
            let mut sample = StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 0.5 };
            let mut touch = |_: &Mesh, _: u32| {};
            stroke.apply_dab(&mut mesh, sample, &mut touch);
            let first = mesh.positions[center].y;
            stroke.apply_dab(&mut mesh, sample, &mut touch);
            assert!((mesh.positions[center].y - first).abs() < 1e-6);
            sample.pressure = 1.0;
            stroke.apply_dab(&mut mesh, sample, &mut touch);
            let stronger = mesh.positions[center].y;
            assert!(((stronger - 1.0).abs() - 2.0 * (first - 1.0).abs()).abs() < 1e-5);
            let mut next = Stroke::new(settings, Symmetry::default(), invert);
            next.apply_dab(&mut mesh, sample, &mut touch);
            assert!((mesh.positions[center].y - 1.0).abs() > (stronger - 1.0).abs());

            let mut mesh = base;
            let mut accumulating =
                Stroke::new(BrushSettings { accumulate: true, ..settings }, Symmetry::default(), invert);
            accumulating.apply_dab(&mut mesh, sample, &mut touch);
            accumulating.apply_dab(&mut mesh, sample, &mut touch);
            assert!((mesh.positions[center].y - 1.0).abs() > (stronger - 1.0).abs());
        }
    }

    #[test]
    fn accumulate_is_recorded_and_legacy_recipes_keep_the_default() {
        for accumulate in [true, false] {
            let mut mesh = oxy_mesh::primitives::icosphere(3, 1.0);
            let settings = BrushSettings { accumulate, ..Default::default() };
            let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_recording();
            stroke.add_sample(
                &mut mesh,
                StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 1.0 },
                &mut |_, _| {},
            );
            let bytes = stroke.take_recipe().unwrap().to_bytes();
            assert_eq!(bytes[4], if accumulate { 4 } else { 5 });
            assert_eq!(StrokeRecipe::from_bytes(&bytes, &|_| None).unwrap().settings.accumulate, accumulate);
        }
    }

    #[test]
    fn strength_and_size_follow_their_own_pressure_curves() {
        // #305: the same light stroke with a steep size curve makes a wider
        // dab than with a flat one, at the same strength, and the other
        // way round.
        let base = oxy_mesh::primitives::icosphere(5, 1.0);
        let run = |curves: [[f32; 4]; 2]| {
            let settings = BrushSettings {
                kind: BrushKind::Draw,
                radius: 0.25,
                strength: 0.5,
                pressure_radius: true,
                pressure_curves: curves,
                ..Default::default()
            };
            let mut mesh = base.clone();
            let mut stroke = Stroke::new(settings, Symmetry::default(), false);
            let p = Vec3::Y;
            stroke.add_sample(&mut mesh, StrokeSample { position: p, normal: p, pressure: 0.3 }, &mut |_, _| {});
            let peak = mesh.positions.iter().zip(&base.positions).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
            // The dab's footprint, relative to its own height.
            let moved =
                mesh.positions.iter().zip(&base.positions).filter(|(a, b)| a.distance(**b) > 0.01 * peak).count();
            (moved, peak)
        };
        let (soft, firm) = ([0.1, 0.9, 0.2, 1.0], [0.9, 0.0, 1.0, 0.1]);
        let (wide, narrow) = (run([LINEAR_PRESSURE_SHAPE, soft]), run([LINEAR_PRESSURE_SHAPE, firm]));
        assert!(wide.0 > narrow.0 * 2, "size follows its curve: {wide:?} {narrow:?}");
        let (strong, weak) = (run([soft, LINEAR_PRESSURE_SHAPE]), run([firm, LINEAR_PRESSURE_SHAPE]));
        assert!(strong.0.abs_diff(weak.0) * 10 <= strong.0, "same size: {strong:?} {weak:?}");
        assert!(strong.1 > weak.1 * 2.0, "strength follows its curve: {strong:?} {weak:?}");
    }

    #[test]
    fn movement_brush_defaults_are_moderate_and_front_face_filter_is_off() {
        for kind in [
            BrushKind::Grab,
            BrushKind::SnakeHook,
            BrushKind::TopoMove,
            BrushKind::KelvinGrab,
            BrushKind::KelvinTwist,
            BrushKind::KelvinScale,
            BrushKind::CurveMove,
        ] {
            assert_eq!(kind.default_strength(), 0.5, "{kind:?}");
        }
        assert!(!BrushSettings::default().front_faces_only);
    }

    #[test]
    fn every_insert_shape_sits_on_its_base_facing_up() {
        for shape in InsertShape::ALL {
            let m = shape.build_mesh();
            // From the points: `bounds()` is cached from before the shift up.
            let b = oxy_mesh::Aabb::from_points(m.positions.iter().copied());
            assert!(b.min.y > -0.01, "{shape:?} sits on Y = 0: {b:?}");
            assert!(b.max.y > 0.0 || shape == InsertShape::Plane, "{shape:?}");
            assert!((b.min.x + b.max.x).abs() < 0.05 && (b.min.z + b.max.z).abs() < 0.05, "{shape:?} centred");
            assert_eq!(preset::shape_from_key(preset::shape_key(shape)), Some(shape));
        }
        assert!(InsertShape::QuadSphere.build_mesh().quad_count() >= 500, "quads");
    }

    #[test]
    fn modulation_scales_dab_size_and_strength() {
        let base = BrushSettings { kind: BrushKind::Draw, radius: 0.3, strength: 0.5, ..Default::default() };
        let mut s = Stroke::new(base, Symmetry::default(), false);
        let sample = StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 1.0 };
        let plain = s.make_dab(sample);
        s.set_modulation(2.0, 0.25);
        let scaled = s.make_dab(sample);
        assert!((scaled.radius - plain.radius * 2.0).abs() < 1e-6);
        assert!((scaled.strength - plain.strength * 0.25).abs() < 1e-6);
        // Applies even when pen pressure doesn't drive strength.
        let mut t = Stroke::new(BrushSettings { pressure_strength: false, ..base }, Symmetry::default(), false);
        t.set_modulation(1.0, 0.0);
        assert_eq!(t.make_dab(sample).strength, 0.0);
    }
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

    /// A short stroke across the side of a sphere (on +X), facing +X.
    fn side_stroke(settings: BrushSettings, sym: Symmetry) -> (Mesh, Mesh) {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let mut s = Stroke::new(settings, sym, false);
        for i in 0..8 {
            let z = -0.2 + i as f32 * 0.05;
            let p = Vec3::new(1.0, 0.0, z).normalize();
            s.add_sample(&mut mesh, StrokeSample { position: p, normal: p, pressure: 1.0 }, &mut |_, _| {});
        }
        (before, mesh)
    }

    #[test]
    fn gravity_sags_the_stroke_downward() {
        let base = BrushSettings { radius: 0.3, strength: 0.5, ..Default::default() };
        let (b, plain) = side_stroke(base, Symmetry::default());
        let (_, heavy) = side_stroke(BrushSettings { gravity: 1.0, ..base }, Symmetry::default());
        // Gravity off: exactly the brush as before.
        let (_, off) = side_stroke(BrushSettings { gravity: 0.0, ..base }, Symmetry::default());
        assert_eq!(plain.positions, off.positions);
        // On: the touched points end lower than without it, never higher,
        // and points far from the stroke don't move.
        let sum_y = |m: &Mesh| m.positions.iter().map(|p| p.y).sum::<f32>();
        assert!(sum_y(&heavy) < sum_y(&plain) - 0.01, "{} vs {}", sum_y(&heavy), sum_y(&plain));
        for ((p0, p1), p2) in b.positions.iter().zip(&plain.positions).zip(&heavy.positions) {
            assert!(p2.y <= p1.y + 1e-4, "a point rose: {p1} -> {p2}");
            if p0.distance(Vec3::X) > 0.7 {
                assert_eq!(p0, p2, "far point moved");
            }
        }
        // Mask protects it too.
        let mut mesh = primitives::icosphere(4, 1.0);
        mesh.mask.iter_mut().for_each(|m| *m = 1.0);
        let before = mesh.positions.clone();
        let mut s = Stroke::new(BrushSettings { gravity: 1.0, ..base }, Symmetry::default(), false);
        s.add_sample(&mut mesh, StrokeSample { position: Vec3::X, normal: Vec3::X, pressure: 1.0 }, &mut |_, _| {});
        assert_eq!(before, mesh.positions);
        // Lighter pen pressure, less sag.
        let sag = |pressure: f32| {
            let mut mesh = primitives::icosphere(4, 1.0);
            let s0 = sum_y(&mesh);
            let mut s = Stroke::new(
                BrushSettings { gravity: 1.0, kind: BrushKind::Smooth, ..base },
                Symmetry::default(),
                false,
            );
            s.add_sample(&mut mesh, StrokeSample { position: Vec3::X, normal: Vec3::X, pressure }, &mut |_, _| {});
            s0 - sum_y(&mesh)
        };
        assert!(sag(0.3) < sag(1.0) * 0.5, "{} vs {}", sag(0.3), sag(1.0));
    }

    #[test]
    fn gravity_drapes_deeply() {
        // One pass of Draw (strength 0.5) across the side of a sphere, one
        // brush-width each way of the point under the brush. The pass stays
        // on the original surface, so points that sag out of the brush stop
        // sagging (in the app the brush follows the surface down).
        let pass = |gravity: f32| {
            let mut mesh = primitives::icosphere(5, 1.0);
            let r = 0.3;
            let settings = BrushSettings { radius: r, strength: 0.5, gravity, ..Default::default() };
            let mut s = Stroke::new(settings, Symmetry::default(), false);
            for i in 0..=40 {
                let z = -r + 2.0 * r * i as f32 / 40.0;
                let p = Vec3::new(1.0, 0.0, z).normalize();
                s.add_sample(&mut mesh, StrokeSample { position: p, normal: p, pressure: 1.0 }, &mut |_, _| {});
            }
            // How far the point that started under the middle of the pass went down.
            let start = primitives::icosphere(5, 1.0);
            let v = (0..start.vertex_count()).min_by(|&a, &b| {
                start.positions[a].distance_squared(Vec3::X).total_cmp(&start.positions[b].distance_squared(Vec3::X))
            });
            let v = v.unwrap();
            (start.positions[v].y - mesh.positions[v].y) / r
        };
        // Gravity 0.3 now is what 1 used to be (0.3 radius per pass).
        let (old_full, half, full) = (pass(0.3), pass(0.5), pass(1.0));
        eprintln!("sag: old 1 = {old_full:.2} R, 0.5 = {half:.2} R, 1 = {full:.2} R");
        assert!(half > old_full, "0.5 is already more than the old full strength: {half:.2} vs {old_full:.2}");
        assert!(full > 2.0 * old_full, "full strength is far more: {full:.2} vs {old_full:.2}");
        assert!(full > 0.5, "a deep drape at 1: {full:.2} radii");
    }

    #[test]
    fn gravity_keeps_mirrored_strokes_symmetric_and_skips_mask_and_paint() {
        let base = BrushSettings { radius: 0.3, strength: 0.5, gravity: 1.0, ..Default::default() };
        let (_, m) = side_stroke(base, Symmetry { x: true, ..Default::default() });
        // X mirror: down stays down on both sides, and the result is mirrored.
        let b = m.bounds();
        assert!((b.max.x + b.min.x).abs() < 1e-3, "{b:?}");
        let low = |side: f32| m.positions.iter().filter(|p| p.x * side > 0.8).map(|p| p.y).fold(f32::MAX, f32::min);
        assert!((low(1.0) - low(-1.0)).abs() < 1e-3, "{} vs {}", low(1.0), low(-1.0));
        // Mask and Paint brushes never move points, gravity or not.
        for kind in [BrushKind::Mask, BrushKind::Paint] {
            let (b, a) = side_stroke(BrushSettings { kind, ..base }, Symmetry::default());
            assert_eq!(b.positions, a.positions, "{kind:?}");
        }
        // Layer (its own pass) and mirrored Snake Hook and Pinch (blended
        // copies) sag too, and stay mirror images.
        let sum_y = |m: &Mesh| m.positions.iter().map(|p| p.y).sum::<f32>();
        for kind in [BrushKind::Layer, BrushKind::SnakeHook, BrushKind::Pinch] {
            let sym = Symmetry { x: true, ..Default::default() };
            let (_, plain) = side_stroke(BrushSettings { kind, gravity: 0.0, ..base }, sym);
            let (_, heavy) = side_stroke(BrushSettings { kind, ..base }, sym);
            assert!(sum_y(&heavy) < sum_y(&plain) - 1e-3, "{kind:?}: {} vs {}", sum_y(&heavy), sum_y(&plain));
            let b = heavy.bounds();
            assert!((b.max.x + b.min.x).abs() < 1e-3, "{kind:?} {b:?}");
        }
        assert!(!BrushKind::Grab.uses_gravity() && !BrushKind::Mask.uses_gravity());
        assert!(BrushKind::Draw.uses_gravity() && BrushKind::SnakeHook.uses_gravity());
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

    fn shifted_sphere(offset: Vec3) -> Mesh {
        let mut data = primitives::icosphere(4, 1.0).to_data();
        for p in &mut data.positions {
            *p += offset;
        }
        Mesh::from_data(data)
    }

    #[test]
    fn grab_mirrors_about_the_symmetry_center() {
        // A sphere centred at x = 0.4: mirroring about the world origin would
        // send the copy into empty space and move nothing on the other side.
        let c = Vec3::new(0.4, 0.0, 0.0);
        let mesh0 = shifted_sphere(c);
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.3, strength: 1.0, ..Default::default() };
        let right = StrokeSample { position: c + Vec3::X, normal: Vec3::X, pressure: 1.0 };
        let drag = StrokeSample { position: c + Vec3::X * 1.4, ..right };
        let run = |center: Vec3| {
            let mut mesh = mesh0.clone();
            let mut s =
                Stroke::new(settings, Symmetry { x: true, ..Default::default() }, false).with_symmetry_center(center);
            s.add_sample(&mut mesh, right, &mut |_, _| {});
            s.add_sample(&mut mesh, drag, &mut |_, _| {});
            mesh.bounds()
        };
        let world = run(Vec3::ZERO);
        assert!(world.min.x > c.x - 1.0 - 1e-3, "world origin: left side untouched {world:?}");
        let own = run(c);
        assert!(own.max.x > c.x + 1.3 && own.min.x < c.x - 1.3, "{own:?}");
        assert!((own.max.x - c.x + (own.min.x - c.x)).abs() < 1e-3, "mirrored evenly: {own:?}");
    }

    #[test]
    fn estimated_symmetry_center_prefers_the_origin() {
        let sphere = primitives::icosphere(3, 1.0);
        assert_eq!(estimate_symmetry_center(&sphere.positions), Vec3::ZERO);
        // Off-centre and symmetric: the shape's own middle.
        let off = shifted_sphere(Vec3::new(0.4, -2.0, 7.5));
        let c = estimate_symmetry_center(&off.positions);
        assert!((c - Vec3::new(0.4, -2.0, 7.5)).abs().max_element() < 0.05, "{c}");
        // Centred but lopsided (an "arm" on one side): stays at the origin,
        // where the bounds' middle would have moved the plane.
        let mut lopsided = sphere.positions.clone();
        lopsided.extend((0..200).map(|i| Vec3::new(1.0 + i as f32 * 0.01, 0.0, 0.0)));
        assert_eq!(estimate_symmetry_center(&lopsided).x, 0.0);
        assert_eq!(estimate_symmetry_center(&[]), Vec3::ZERO);
        assert_eq!(estimate_symmetry_center(&[Vec3::splat(3.0)]), Vec3::ZERO);
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
    fn grab_strength_controls_displacement() {
        let mesh0 = primitives::icosphere(4, 1.0);
        let sample0 = top_sample();
        let sample1 = StrokeSample { position: Vec3::Y * 1.5, ..sample0 };

        // Strength 0.0 should not move vertices at all
        let mut mesh_zero = mesh0.clone();
        let settings_zero = BrushSettings { kind: BrushKind::Grab, radius: 0.3, strength: 0.0, ..Default::default() };
        let mut s_zero = Stroke::new(settings_zero, Symmetry::default(), false);
        s_zero.add_sample(&mut mesh_zero, sample0, &mut |_, _| {});
        s_zero.add_sample(&mut mesh_zero, sample1, &mut |_, _| {});
        assert_eq!(mesh_zero.positions, mesh0.positions);

        // Strength 0.5 should move exactly half as much as strength 1.0
        let mut mesh_half = mesh0.clone();
        let settings_half = BrushSettings { kind: BrushKind::Grab, radius: 0.3, strength: 0.5, ..Default::default() };
        let mut s_half = Stroke::new(settings_half, Symmetry::default(), false);
        s_half.add_sample(&mut mesh_half, sample0, &mut |_, _| {});
        s_half.add_sample(&mut mesh_half, sample1, &mut |_, _| {});

        let mut mesh_full = mesh0.clone();
        let settings_full = BrushSettings { kind: BrushKind::Grab, radius: 0.3, strength: 1.0, ..Default::default() };
        let mut s_full = Stroke::new(settings_full, Symmetry::default(), false);
        s_full.add_sample(&mut mesh_full, sample0, &mut |_, _| {});
        s_full.add_sample(&mut mesh_full, sample1, &mut |_, _| {});

        let peak_idx = mesh0.positions.iter().enumerate().max_by(|(_, a), (_, b)| a.y.total_cmp(&b.y)).unwrap().0;
        let d_half = mesh_half.positions[peak_idx].y - mesh0.positions[peak_idx].y;
        let d_full = mesh_full.positions[peak_idx].y - mesh0.positions[peak_idx].y;
        assert!(d_full > 0.4);
        assert!(
            (d_half - d_full * 0.5).abs() < 1e-4,
            "half strength should produce half displacement: {d_half} vs {d_full}"
        );
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

    /// A straight stroke along X over a dense flat grid (z = 0, normal +Z),
    /// with brush `kind`. Returns the grid before and after.
    fn line_on_grid(kind: BrushKind, invert: bool) -> (Mesh, Mesh) {
        let mut mesh = primitives::plane(160, 2.0);
        let before = mesh.clone();
        let n = mesh.normals[0];
        let settings = BrushSettings { kind, radius: 0.2, strength: 1.0, spacing: 0.1, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), invert);
        for i in 0..=30 {
            let p = Vec3::new(-0.6 + 0.04 * i as f32, 0.0, 0.0);
            let p = if n.y.abs() > 0.5 { Vec3::new(p.x, 0.0, p.y) } else { p };
            s.add_sample(&mut mesh, StrokeSample { position: p, normal: n, pressure: 1.0 }, &mut |_, _| {});
        }
        (before, mesh)
    }

    /// Height (along the plane's normal) of the grid points near x = 0,
    /// by their distance across the stroke, after `line_on_grid`.
    fn cross_section(before: &Mesh, after: &Mesh) -> Vec<(f32, f32)> {
        let n = before.normals[0];
        let across = if n.y.abs() > 0.5 { Vec3::Z } else { Vec3::Y };
        let mut out: Vec<(f32, f32)> = before
            .positions
            .iter()
            .zip(&after.positions)
            .filter(|(b, _)| b.x.abs() < 0.02)
            .map(|(b, a)| (b.dot(across), (a - b).dot(n)))
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    #[test]
    fn sharp_crease_cuts_a_v_along_the_stroke() {
        let (b, a) = line_on_grid(BrushKind::SharpCrease, false);
        let n = b.normals[0];
        let sec = cross_section(&b, &a);
        let depth_at =
            |d: f32| sec.iter().min_by(|x, y| (x.0 - d).abs().total_cmp(&(y.0 - d).abs())).map(|x| x.1).unwrap();
        let bottom = depth_at(0.0);
        assert!(bottom < -0.004, "it cuts in: {bottom}");
        // Deepest on the line and shallower out to the sides.
        assert!(depth_at(0.05) > bottom && depth_at(0.1) > depth_at(0.05), "{sec:?}");
        // Points beside the line moved toward it, across the stroke only,
        // and none crossed it.
        let across = if n.y.abs() > 0.5 { Vec3::Z } else { Vec3::Y };
        let along = Vec3::X;
        for (p, q) in b.positions.iter().zip(&a.positions) {
            if p.x.abs() > 0.2 {
                continue;
            }
            let (c0, c1) = (p.dot(across), q.dot(across));
            if c0.abs() > 1e-4 {
                assert!(c1 * c0 >= -1e-6, "a point crossed the line: {c0} -> {c1}");
                assert!(c1.abs() <= c0.abs() + 1e-5, "a point moved away from the line");
            }
            // Across, not along: what little moves along comes from the
            // normal tilting as the groove forms.
            let d = q - p;
            assert!(d.dot(along).abs() <= 0.05 * d.length() + 1e-5, "along {} of {} at {p}", d.dot(along), d.length());
        }
        // Compared with Crease, its groove is narrower for its depth.
        let (cb, ca) = line_on_grid(BrushKind::Crease, false);
        let csec = cross_section(&cb, &ca);
        let width = |sec: &[(f32, f32)]| {
            let deep = sec.iter().map(|x| x.1).fold(0.0f32, f32::min);
            sec.iter().filter(|x| x.1 < deep * 0.5).map(|x| x.0.abs()).fold(0.0f32, f32::max)
        };
        assert!(width(&sec) < width(&csec), "sharp {} vs crease {}", width(&sec), width(&csec));
        // Inverted, it raises a ridge.
        let (b, a) = line_on_grid(BrushKind::SharpCrease, true);
        let sec = cross_section(&b, &a);
        assert!(sec.iter().map(|x| x.1).fold(0.0f32, f32::max) > 0.004);
    }

    #[test]
    fn crease_pinch_parameter_controls_gather() {
        let mesh0 = primitives::icosphere(5, 1.0);
        let dab_pos = Vec3::new(0.0, 0.0, 1.0);

        let stroke_with_pinch = |pinch: f32| {
            let mut mesh = mesh0.clone();
            let settings = BrushSettings {
                kind: BrushKind::Crease,
                radius: 0.3,
                strength: 1.0,
                crease_pinch: pinch,
                ..Default::default()
            };
            let mut s = Stroke::new(settings, Symmetry::default(), false);
            s.add_sample(&mut mesh, StrokeSample { position: dab_pos, normal: Vec3::Z, pressure: 1.0 }, &mut |_, _| {});
            mesh
        };

        let mesh_no_pinch = stroke_with_pinch(0.0);
        let mesh_high_pinch = stroke_with_pinch(1.0);

        // A vertex slightly offset from center (e.g. at x = 0.1)
        // With high pinch, it should be drawn closer to the center line than with zero pinch.
        assert_ne!(mesh_no_pinch.positions, mesh_high_pinch.positions);
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
            invert: false,
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
    fn alpha_invert_flips_dab_displacement() {
        let mut normal_plane = primitives::plane(60, 2.0);
        let mut inverted_plane = normal_plane.clone();
        let settings = BrushSettings { radius: 0.5, strength: 1.0, falloff: Falloff::Constant, ..Default::default() };
        let sample = StrokeSample { position: Vec3::ZERO, normal: Vec3::Y, pressure: 1.0 };
        let alpha = Arc::new(BuiltinAlpha::Bumps.generate(64));
        let stamp_normal =
            AlphaStamp { alpha: alpha.clone(), rotation: 0.0, follow_stroke: false, invert: false, up: Vec3::Z };
        let stamp_inverted = AlphaStamp { alpha, rotation: 0.0, follow_stroke: false, invert: true, up: Vec3::Z };
        Stroke::new(settings, Symmetry::default(), false).with_alpha(Some(stamp_normal)).add_sample(
            &mut normal_plane,
            sample,
            &mut |_, _| {},
        );
        Stroke::new(settings, Symmetry::default(), false).with_alpha(Some(stamp_inverted)).add_sample(
            &mut inverted_plane,
            sample,
            &mut |_, _| {},
        );
        let yn = normal_plane.positions.iter().find(|p| p.x.abs() < 0.05 && p.z.abs() < 0.05).map(|p| p.y).unwrap();
        let yi = inverted_plane.positions.iter().find(|p| p.x.abs() < 0.05 && p.z.abs() < 0.05).map(|p| p.y).unwrap();
        assert!(yn != yi, "normal height ({yn}) and inverted height ({yi}) should differ");
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
    fn grab_symmetry_overlap_boundary_is_smooth_without_creases_or_steps() {
        let xsym = Symmetry { x: true, ..Default::default() };
        let mut mesh = primitives::icosphere(5, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings {
            kind: BrushKind::Grab,
            radius: 0.5,
            strength: 1.0,
            front_faces_only: true,
            ..Default::default()
        };
        let mut s = Stroke::new(settings, xsym, false).with_view(-Vec3::Z);
        let p0 = Vec3::new(0.25, 0.0, (1.0 - 0.25 * 0.25_f32).sqrt());
        s.add_sample(&mut mesh, StrokeSample { position: p0, normal: p0.normalize(), pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(
            &mut mesh,
            StrokeSample { position: p0 + Vec3::new(-0.3, -0.2, 0.0), normal: p0.normalize(), pressure: 1.0 },
            &mut |_, _| {},
        );
        assert!(max_edge_stretch(&before, &mesh) < 2.5);
        assert_closed_seam(&before, &mesh);
        assert!(max_image_gap(&mesh, |p| Vec3::new(-p.x, p.y, p.z)) < 1e-4);
    }

    #[test]
    fn front_faces_only_fades_instead_of_cutting() {
        assert_eq!(front_fade(1.0), 1.0);
        assert_eq!(front_fade(FRONT_FADE), 1.0);
        assert_eq!(front_fade(0.0), 0.0);
        let mut last = 0.0;
        for i in 1..=100 {
            let w = front_fade(i as f32 / 100.0);
            assert!(w >= last && w - last < 0.06, "jump at {}", i as f32 / 100.0);
            last = w;
        }
        // Grab near the silhouette with the brush facing the viewer: the
        // surface inside the brush turns past sideways. The hard cut moved
        // one vertex fully and left the next (about 5× an edge length);
        // the fade keeps neighbours together.
        let mut mesh = primitives::icosphere(5, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings {
            kind: BrushKind::Grab,
            radius: 0.5,
            strength: 1.0,
            front_faces_only: true,
            ..Default::default()
        };
        assert!(settings.front_faces_only);
        let p0 = Vec3::new(0.97, 0.0, (1.0 - 0.97f32 * 0.97).sqrt());
        let mut s = Stroke::new(settings, Symmetry::default(), false).with_view(-Vec3::Z);
        s.add_sample(&mut mesh, StrokeSample { position: p0, normal: Vec3::Z, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(
            &mut mesh,
            StrokeSample { position: p0 - Vec3::Y * 0.3, normal: Vec3::Z, pressure: 1.0 },
            &mut |_, _| {},
        );
        let d = |v: usize| mesh.positions[v] - before.positions[v];
        let mut worst = 0.0f32;
        for t in &before.triangles {
            for i in 0..3 {
                let (a, b) = (t[i] as usize, t[(i + 1) % 3] as usize);
                let l = before.positions[a].distance(before.positions[b]).max(1e-6);
                worst = worst.max((d(a) - d(b)).length() / l);
            }
        }
        assert!(worst < 2.5, "displacement jumps {worst:.2}× an edge across one edge");
    }

    #[test]
    fn the_overlap_divisor_counts_one_brush_once() {
        // One copy, or copies that don't reach each other: unchanged.
        for a in [0.0, 0.3, 1.0] {
            assert_eq!(overlap_divisor(a, a, 0.7), 1.0);
            assert_eq!(overlap_divisor(a + 0.2, a.max(0.2), 0.0), 1.0);
        }
        // Under the cursor (full weight, the mirror copy at its overlap): the
        // sum 1 + overlap comes back to exactly one drag.
        for o in [0.05, 0.3, 0.65, 1.0] {
            let d = overlap_divisor(1.0 + o, 1.0, o);
            assert!(((1.0 + o) / d - 1.0).abs() < 1e-6, "overlap {o}: {d}");
        }
        // Two copies on top of each other: one brush, at every weight.
        for w in [0.1, 0.5, 1.0] {
            assert!((2.0 * w / overlap_divisor(2.0 * w, w, 1.0) - w).abs() < 1e-6);
        }
        // Continuous as the mirror copy's share grows from nothing, with no
        // step where it starts to reach, and never below one.
        let mut last = overlap_divisor(0.6, 0.6, 0.4);
        for k in 1..=200 {
            let b = k as f32 / 200.0 * 0.6;
            let d = overlap_divisor(0.6 + b, 0.6, 0.4);
            assert!(d >= 1.0 && (d - last).abs() < 0.01, "jump at {b}: {last} -> {d}");
            last = d;
        }
    }

    /// The plane stroke of `plane_stroke`, with the brush centre `x` from
    /// the X mirror plane.
    fn pull_at(kind: BrushKind, sym: Symmetry, x: f32) -> (Mesh, Mesh) {
        let mut mesh = primitives::plane(60, 2.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.5, strength: 1.0, spacing: 0.2, ..Default::default() };
        let mut s = Stroke::new(settings, sym, false).with_view(-Vec3::Y);
        let start = Vec3::new(x, 0.0, 0.0);
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: Vec3::Y, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(
            &mut mesh,
            StrokeSample { position: start + Vec3::Y * 0.3, normal: Vec3::Y, pressure: 1.0 },
            &mut |_, _| {},
        );
        (before, mesh)
    }

    #[test]
    fn grabbing_on_the_mirror_plane_is_one_round_brush_not_a_mesa() {
        // The report on 4.0.0-beta.21: "the points are capped, so it isn't
        // round". Pulling on the plane, both copies are the same brush; the
        // cap made a flat top there. Now it is exactly the brush itself.
        let xsym = Symmetry { x: true, ..Default::default() };
        for kind in [BrushKind::Grab, BrushKind::TopoMove, BrushKind::KelvinGrab] {
            let (before, sym) = pull_at(kind, xsym, 0.0);
            let (_, solo) = pull_at(kind, Symmetry::default(), 0.0);
            for (i, p) in before.positions.iter().enumerate() {
                let (a, b) = (sym.positions[i].y - p.y, solo.positions[i].y - p.y);
                assert!((a - b).abs() < 1e-4, "{kind:?} at {p}: {a} with symmetry, {b} without");
            }
        }
        // A little off the plane the top stays round: no wider at its peak
        // than the brush alone, and the point under the cursor follows it.
        for kind in [BrushKind::Grab, BrushKind::TopoMove] {
            for x in [1.0 / 30.0, 2.0 / 30.0] {
                let (before, sym) = pull_at(kind, xsym, x);
                let rise: Vec<(Vec3, f32)> =
                    before.positions.iter().zip(&sym.positions).map(|(p, q)| (*p, q.y - p.y)).collect();
                let peak = rise.iter().map(|r| r.1).fold(0.0f32, f32::max);
                let top = rise.iter().filter(|r| r.1 >= peak - 0.005 * 0.3).count();
                let (b0, solo) = pull_at(kind, Symmetry::default(), 0.0);
                let solo_top =
                    b0.positions.iter().zip(&solo.positions).filter(|(p, q)| q.y - p.y >= 0.3 * 0.995).count();
                assert!(
                    top <= solo_top + 4,
                    "{kind:?} at {x}: a flat top of {top} points (the brush alone: {solo_top})"
                );
                let under = vertex_at(&before, Vec3::new(x, 0.0, 0.0));
                let follow = sym.positions[under].y - before.positions[under].y;
                assert!((follow - 0.3).abs() < 0.02 * 0.3 + 1e-4, "{kind:?} at {x}: under the cursor {follow}");
                assert!(peak <= 0.3 * 1.2, "{kind:?} at {x}: ran far ahead of the drag ({peak})");
            }
        }
    }

    /// Rise of the seam vertex and of the brush centre for a pull along the
    /// normal with the brush just off the X mirror plane (`plane_stroke`).
    fn seam_and_centre_rise(kind: BrushKind, sym: Symmetry) -> (f32, f32) {
        let (before, after) = plane_stroke(kind, sym, false, Vec3::Y * 0.3);
        let origin = vertex_at(&before, Vec3::ZERO);
        let centre = vertex_at(&before, Vec3::new(0.2, 0.0, 0.0));
        (after.positions[origin].y - before.positions[origin].y, after.positions[centre].y - before.positions[centre].y)
    }

    #[test]
    fn grab_family_overlap_on_the_plane_adds_up_without_a_valley() {
        // The reported bug: pulling out near the mirror plane made two bumps
        // with a valley between them, because the plane only got one copy's
        // weak edge weight. Every grab-like brush now rises in one piece.
        let xsym = Symmetry { x: true, ..Default::default() };
        for kind in [BrushKind::Grab, BrushKind::TopoMove, BrushKind::KelvinGrab, BrushKind::SnakeHook] {
            let (seam, centre) = seam_and_centre_rise(kind, xsym);
            assert!(centre > 0.05, "{kind:?} centre barely moved: {centre}");
            // Kelvin Grab's own Kelvinlet profile falls off with distance
            // from the centre (part of its feel), so it keeps a little less.
            let min_ratio = if kind == BrushKind::KelvinGrab { 0.85 } else { 0.95 };
            assert!(seam >= min_ratio * centre, "{kind:?} valley on the plane: seam {seam} vs centre {centre}");
            // Two overlapping pulls rise a little more between them than at
            // either brush, never far past the drag.
            assert!(seam <= centre * 1.2, "{kind:?} seam ran far ahead of the brush: {seam} vs {centre}");
            // Without symmetry the same spot gets just the falloff: the
            // overlap is what lifts it.
            let (solo_seam, _) = seam_and_centre_rise(kind, Symmetry::default());
            assert!(seam > solo_seam * 1.2, "{kind:?} overlap did not add: {seam} vs solo {solo_seam}");
        }
    }

    #[test]
    fn grab_family_without_overlap_matches_a_single_brush() {
        // Away from the plane (no overlap) each brush keeps its own feel:
        // the symmetric stroke's primary side equals the same stroke with
        // symmetry off, for every variant.
        let xsym = Symmetry { x: true, ..Default::default() };
        for kind in [
            BrushKind::Grab,
            BrushKind::TopoMove,
            BrushKind::KelvinGrab,
            BrushKind::KelvinTwist,
            BrushKind::KelvinScale,
            BrushKind::SnakeHook,
        ] {
            let run = |sym: Symmetry| {
                let mut mesh = primitives::icosphere(4, 1.0);
                let settings = BrushSettings { kind, radius: 0.25, strength: 1.0, ..Default::default() };
                let mut s = Stroke::new(settings, sym, false).with_view(-Vec3::X);
                let start = Vec3::X;
                s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
                s.add_sample(
                    &mut mesh,
                    StrokeSample { position: start + Vec3::new(0.2, 0.1, 0.0), normal: start, pressure: 1.0 },
                    &mut |_, _| {},
                );
                mesh
            };
            let (with, solo) = (run(xsym), run(Symmetry::default()));
            for (a, b) in with.positions.iter().zip(&solo.positions) {
                if b.x > 0.0 {
                    assert!(a.distance(*b) < 1e-5, "{kind:?} changed away from the plane: {a} vs {b}");
                }
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
        // On the plane both copies agree, and their weights add up, less the
        // part that counts the same brush twice: more than one copy's falloff
        // (the old valley), and not flattened at one drag (the old cap).
        let overlap = Falloff::Smooth.eval(0.4 / 0.5);
        let expected = 0.3 * 2.0 * w_seam / overlap_divisor(2.0 * w_seam, w_seam, overlap);
        assert!(expected > 0.3 * w_seam * 1.3);
        assert!(seam_move.distance(Vec3::new(0.0, 0.0, expected)) < 1e-4, "{seam_move} vs {expected}");
        // The brush centre is inside the mirror sphere too. Parallel to the
        // plane, the stronger copy wins, so it matches a brush with no mirror.
        let (_, solo) = plane_stroke(BrushKind::Grab, Symmetry::default(), false, Vec3::new(0.0, 0.0, 0.3));
        assert!(after.positions[centre].distance(solo.positions[centre]) < 1e-4);
        // Outside the overlap region (where the mirror brush does not reach),
        // the positive side matches a solo brush with no symmetry.
        for (i, p0) in before.positions.iter().enumerate() {
            if p0.x >= 0.3 {
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
        // Parallel to the plane, the seam follows the stroke about once:
        // two overlapping copies carry it a little further than the brush
        // centre (here 1.17 times), as overlapping pulls do, not twice.
        let (before, after) = plane_stroke(BrushKind::SnakeHook, sym, false, Vec3::new(0.0, 0.0, 0.2));
        let origin = vertex_at(&before, Vec3::ZERO);
        let centre = vertex_at(&before, Vec3::new(0.2, 0.0, 0.0));
        let seam = (after.positions[origin] - before.positions[origin]).length();
        let at_centre = (after.positions[centre] - before.positions[centre]).length();
        assert!(seam < at_centre * 1.2, "seam ran far ahead of the brush ({seam} > {at_centre})");
        assert!(after.positions[origin].x.abs() < 1e-3);
    }

    fn crease_across_plane(kind: BrushKind, sym: Symmetry) -> (Mesh, Mesh) {
        let mut mesh = primitives::plane(80, 2.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.3, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, sym, false).with_view(-Vec3::Y);
        for k in 0..=40 {
            let x = -0.6 + 1.2 * k as f32 / 40.0;
            let sample = StrokeSample { position: Vec3::new(x, 0.0, 0.0), normal: Vec3::Y, pressure: 1.0 };
            s.add_sample(&mut mesh, sample, &mut |_, _| {});
        }
        (before, mesh)
    }

    #[test]
    fn crease_brushes_have_no_harsh_line_on_the_mirror_plane() {
        // A stroke across the plane used to leave a ridge along it: the
        // plane's vertices were blended down to one copy's depth while their
        // neighbours kept two copies' worth.
        let sym = Symmetry { x: true, ..Default::default() };
        for kind in [BrushKind::Crease, BrushKind::SharpCrease] {
            let (before, after) = crease_across_plane(kind, sym);
            let depth = |x: f32| {
                let i = vertex_at(&before, Vec3::new(x, 0.0, 0.0));
                before.positions[i].y - after.positions[i].y
            };
            let (plane, near) = (depth(0.0), depth(0.1));
            assert!(plane > near * 0.95, "{kind:?} ridge on the plane: {plane} vs {near}");
        }
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

    #[test]
    fn stroke_budget_clamps_spacing_scale_to_prevent_beading() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings =
            BrushSettings { kind: BrushKind::Draw, radius: 0.3, strength: 1.0, spacing: 0.1, ..Default::default() };
        let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_budget(1.0); // 1ms budget
        // Simulate a slow dab (e.g. 50ms per dab as on 83M triangle mesh):
        if let Some(b) = &mut stroke.budget {
            b.record(50.0);
            b.record(50.0);
        }
        let start = Vec3::Y;
        let end = (Vec3::Y + Vec3::X * 0.8).normalize();
        stroke.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
        stroke.add_sample(&mut mesh, StrokeSample { position: end, normal: end, pressure: 1.0 }, &mut |_, _| {});
        // Spacing scale must be clamped to max_spacing_scale (1.5) instead of blowing up to 8+
        assert!(stroke.spacing_scale() <= 1.5, "spacing scale clamped to {}", stroke.spacing_scale());
    }

    /// The sharpest angle (degrees) between neighbouring triangles.
    fn sharpest_fold(m: &Mesh) -> f32 {
        sharpest_fold_where(m, |_, _| true)
    }

    fn sharpest_fold_where(m: &Mesh, edge_in_region: impl Fn(u32, u32) -> bool) -> f32 {
        let n = |t: &[u32; 3]| {
            let [a, b, c] = t.map(|v| m.positions[v as usize]);
            (b - a).cross(c - a).normalize_or_zero()
        };
        let mut by_edge: HashMap<(u32, u32), usize> = HashMap::new();
        let mut worst = 0.0f32;
        for (i, t) in m.triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if !edge_in_region(a, b) {
                    continue;
                }
                if let Some(&j) = by_edge.get(&(b, a)) {
                    worst = worst.max(n(t).dot(n(&m.triangles[j])).clamp(-1.0, 1.0).acos().to_degrees());
                }
                by_edge.insert((a, b), i);
            }
        }
        worst
    }

    /// Pull a sphere's surface `len` along `dir` with `kind`, in 20 samples.
    fn long_pull(kind: BrushKind, dir: Vec3, len: f32) -> Mesh {
        let mut mesh = primitives::icosphere(5, 1.0);
        let contact = Vec3::new(1.0, 1.0, 0.0).normalize();
        let settings = BrushSettings { kind, radius: 0.45, strength: 1.0, ..Default::default() };
        let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_view(Vec3::Z);
        let sample = |p: Vec3| StrokeSample { position: p, normal: contact, pressure: 1.0 };
        stroke.add_sample(&mut mesh, sample(contact), &mut |_, _| {});
        let d = dir.normalize() * len;
        for k in 1..=20 {
            stroke.add_sample(&mut mesh, sample(contact + d * (k as f32 / 20.0)), &mut |_, _| {});
        }
        mesh
    }

    #[test]
    fn kelvin_biscale_horn_comparison() {
        let direction = Vec3::new(1.0, 1.0, 0.0).normalize();
        let base = primitives::icosphere(5, 1.0);
        let mut data = base.to_data();
        // A smooth horn blended into a closed torso. Keep its root inside
        // the support of both brushes so the same pull tests the transition.
        for position in &mut data.positions {
            let cap = ((position.dot(direction) - 0.86) / 0.14).max(0.0);
            *position += direction * (0.42 * cap.powi(3));
        }
        let original = Mesh::from_data(data);
        let tip = original
            .positions
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.dot(direction).total_cmp(&b.dot(direction)))
            .unwrap()
            .0;
        let contact = original.positions[tip];
        let radius = 0.65;
        let run = |kernel, delta: Vec3, tuned| {
            let settings = BrushSettings { kind: BrushKind::KelvinGrab, radius, strength: 1.0, ..Default::default() };
            let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_view(Vec3::Z);
            let mut mesh = original.clone();
            for step in 0..=20 {
                let position = contact + delta * (step as f32 / 20.0);
                let sample = StrokeSample { position, normal: direction, pressure: 1.0 };
                stroke.grab_sample_with_kernel(&mut mesh, sample, &mut |_, _| {}, kernel, tuned);
            }
            assert!(mesh.positions[tip].distance(contact + delta) < 1e-5);
            let torso = original
                .positions
                .iter()
                .zip(&mesh.positions)
                .filter(|(before, _)| before.dot(direction) < 0.88)
                .map(|(before, after)| before.distance(*after))
                .fold(0.0f32, f32::max);
            let root_fold = sharpest_fold_where(&mesh, |a, b| {
                let midpoint = (original.positions[a as usize] + original.positions[b as usize]) * 0.5;
                (0.84..0.99).contains(&midpoint.dot(direction))
            });
            (torso, root_fold, sharpest_fold(&mesh))
        };
        let drag = Vec3::new(-0.35, 0.55, 0.4).normalize();
        for distance in [0.32, 0.65, 1.2, 2.0] {
            let delta = drag * distance;
            let (single_torso, single_root, single_fold) =
                run(kelvin_grab_tensor as fn(Vec3, f32) -> (Mat3, Mat3), delta, false);
            let (bi_torso, bi_root, bi_fold) = run(biscale_grab_tensor, delta, false);
            let (torso, root, fold) = run(kelvin_grab_tensor, delta, true);
            eprintln!("tuned {distance:.2}: torso {torso:.4}, root {root:.1}°, max {fold:.1}°");
            assert!(root <= single_root + 5.0 && fold <= single_fold + 5.0);
            if distance <= radius {
                assert!(torso < 0.9 * single_torso);
            }
            eprintln!(
                "drag {distance:.2}: torso {single_torso:.4} -> {bi_torso:.4}, root fold {single_root:.1}° -> {bi_root:.1}°, max fold {single_fold:.1}° -> {bi_fold:.1}°"
            );
            assert!(bi_torso < 0.75 * single_torso);
            assert!(bi_fold < 90.0, "fold-over at drag {distance}");
        }
    }

    #[test]
    fn kelvin_grab_pulls_long_shapes_without_folding_them() {
        // A tester's horns pulled sideways had a slit from tip to base: the
        // elastic part pinched the surface behind the brush across itself
        // (a 170° fold). Now no sharper than plain Grab.
        for dir in [Vec3::new(-0.3, 1.0, 0.5), Vec3::new(0.5, 1.0, 0.6), Vec3::new(1.0, 1.0, 0.0)] {
            let kelvin = sharpest_fold(&long_pull(BrushKind::KelvinGrab, dir, 1.2));
            let grab = sharpest_fold(&long_pull(BrushKind::Grab, dir, 1.2));
            assert!(
                sharpest_fold(&long_pull(BrushKind::KelvinGrab, dir, 2.0)) < 90.0,
                "no fold-over on a long pull along {dir}"
            );
            assert!(kelvin < 90.0, "no fold-over: {kelvin:.0}° along {dir}");
            assert!(kelvin <= grab + 5.0, "about as smooth as Grab: {kelvin:.0}° vs {grab:.0}° along {dir}");
        }
    }

    #[test]
    fn kelvin_grab_keeps_its_elastic_response_on_small_moves() {
        assert_eq!(elastic_fade(0.0), 1.0);
        assert!(elastic_fade(0.2) > 0.95, "a nudge is fully elastic");
        assert!(elastic_fade(1.0) > 0.85, "a one-radius pull still feels elastic");
        assert!(elastic_fade(8.0) < 0.25, "a very long pull mostly isn't");
        // A nudge sideways: the point behind the brush (opposite the drag)
        // is pulled along more than one beside it at the same distance,
        // which a plain falloff wouldn't do (the Kelvinlet's r r^T term).
        let (_, elastic) = kelvin_grab_tensor(Vec3::new(-0.2, 0.0, 0.0), 0.5);
        let (_, beside) = kelvin_grab_tensor(Vec3::new(0.0, 0.0, 0.2), 0.5);
        let drag = Vec3::X * 0.05;
        assert!((elastic * drag).x > 0.0);
        assert!((elastic * drag).x > (beside * drag).x + 1e-4);
    }

    #[test]
    fn kelvin_affine_drags_work_head_on_reverse_and_return_to_start() {
        let mut original = primitives::icosphere(3, 1.0);
        let contact = Vec3::Z;
        let nearest = original
            .positions
            .iter()
            .enumerate()
            .filter(|(_, p)| (0.15..0.4).contains(&p.distance(contact)))
            .max_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
            .unwrap()
            .0;
        let masked = original
            .positions
            .iter()
            .enumerate()
            .filter(|(i, p)| *i != nearest && (0.15..0.4).contains(&p.distance(contact)))
            .max_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
            .unwrap()
            .0;
        original.mask[masked] = 1.0;
        let offset = original.positions[nearest] - contact;
        for (kind, drag) in [(BrushKind::KelvinTwist, Vec3::X), (BrushKind::KelvinScale, Vec3::Y)] {
            for invert in [false, true] {
                let mut mesh = original.clone();
                let settings = BrushSettings { kind, radius: 0.5, strength: 1.0, ..Default::default() };
                let mut stroke = Stroke::new(settings, Symmetry::default(), invert).with_view(-Vec3::Z);
                let mut sample = |mesh: &mut Mesh, direction| {
                    stroke.add_sample(
                        mesh,
                        StrokeSample { position: contact + direction, normal: Vec3::Z, pressure: 1.0 },
                        &mut |_, _| {},
                    )
                };
                sample(&mut mesh, Vec3::ZERO);
                sample(&mut mesh, drag * 0.25);
                let moved = mesh.positions[nearest] - contact;
                let sign = if invert { -1.0 } else { 1.0 };
                match kind {
                    BrushKind::KelvinTwist => {
                        assert!((moved.length() - offset.length()).abs() < 1e-6);
                        assert!(offset.cross(moved).z * sign > 0.005);
                    }
                    _ => assert!((moved.length() - offset.length()) * sign > 0.01),
                }
                assert_eq!(mesh.positions[masked], original.positions[masked]);
                sample(&mut mesh, Vec3::ZERO);
                assert_eq!(mesh.positions, original.positions);
            }
        }
    }

    #[test]
    fn kelvin_brushes_deform_from_stroke_start() {
        let mut mesh = primitives::icosphere(3, 1.0);
        let contact = mesh.positions[0];
        let normal = mesh.normals[0];
        let delta = Vec3::new(0.2, 0.0, 0.0);

        // 1. KelvinGrab: moves center along delta and preserves volume through Poisson lateral response
        let settings = BrushSettings { kind: BrushKind::KelvinGrab, radius: 0.5, strength: 1.0, ..Default::default() };
        let mut stroke = Stroke::new(settings, Symmetry::default(), false);
        stroke.add_sample(&mut mesh, StrokeSample { position: contact, normal, pressure: 1.0 }, &mut |_, _| {});
        stroke.add_sample(&mut mesh, StrokeSample { position: contact + delta, normal, pressure: 1.0 }, &mut |_, _| {});

        let center_disp = mesh.positions[0] - contact;
        assert!(center_disp.x > 0.15, "center moved: {center_disp:?}");

        // Far vertices must be untouched
        let bottom = mesh.positions.iter().find(|p| p.distance(contact) > 1.2).unwrap();
        let orig_bottom =
            primitives::icosphere(3, 1.0).positions.into_iter().find(|p| p.distance(contact) > 1.2).unwrap();
        assert!((bottom.distance(orig_bottom)).abs() < 1e-4);

        // 2. KelvinTwist: twists around normal
        let mut twist_mesh = primitives::icosphere(3, 1.0);
        let settings_twist =
            BrushSettings { kind: BrushKind::KelvinTwist, radius: 0.5, strength: 1.0, ..Default::default() };
        let mut stroke_twist = Stroke::new(settings_twist, Symmetry::default(), false).with_view(Vec3::Z);
        stroke_twist.add_sample(
            &mut twist_mesh,
            StrokeSample { position: contact, normal, pressure: 1.0 },
            &mut |_, _| {},
        );
        stroke_twist.add_sample(
            &mut twist_mesh,
            StrokeSample { position: contact + Vec3::X * 0.2, normal, pressure: 1.0 },
            &mut |_, _| {},
        );
        // Center vertex should remain very close to contact (axis of twist)
        let center_disp_twist = twist_mesh.positions[0] - contact;
        assert!(center_disp_twist.length() < 0.01, "twist center stayed on axis: {center_disp_twist:?}");
        // Surrounding vertices should have rotated
        let rotated_count = twist_mesh
            .positions
            .iter()
            .zip(&primitives::icosphere(3, 1.0).positions)
            .filter(|(a, b)| a.distance(**b) > 0.005)
            .count();
        assert!(rotated_count > 5, "surrounding vertices twisted: {rotated_count}");

        // 3. KelvinScale: radial displacement
        let mut scale_mesh = primitives::icosphere(3, 1.0);
        let settings_scale =
            BrushSettings { kind: BrushKind::KelvinScale, radius: 0.5, strength: 1.0, ..Default::default() };
        let mut stroke_scale = Stroke::new(settings_scale, Symmetry::default(), false).with_view(Vec3::Z);
        stroke_scale.add_sample(
            &mut scale_mesh,
            StrokeSample { position: contact, normal, pressure: 1.0 },
            &mut |_, _| {},
        );
        stroke_scale.add_sample(
            &mut scale_mesh,
            StrokeSample { position: contact + Vec3::Y * 0.2, normal, pressure: 1.0 },
            &mut |_, _| {},
        );
        let scaled_count = scale_mesh
            .positions
            .iter()
            .zip(&primitives::icosphere(3, 1.0).positions)
            .filter(|(a, b)| a.distance(**b) > 0.005)
            .count();
        assert!(scaled_count > 5, "surrounding vertices scaled: {scaled_count}");
    }
}
