//! Whole-piece deformers: bend, twist, taper, smooth, polish and the like,
//! applied to every point at once and held back by the mask (RFC 0016).
//!
//! A [`Deformation`] is prepared once from a piece (its points, normals,
//! neighbours, groups, the per-point weights `1 - mask` and the bounding box
//! of what can move). [`Deformation::apply`] then works out new positions
//! for any deformer, axes and amount from those original points, so the app
//! can show a live preview while a slider is dragged and recompute from
//! scratch each frame.
//!
//! Conventions, for every deformer:
//! - `amount` runs from -1 to 1 (the slider's -100 to 100). 0 changes
//!   nothing; the sign picks the direction.
//! - `axes` says which axes take part. Deformers that work *along* an axis
//!   (bend, twist, taper, …) are applied once per chosen axis, X then Y
//!   then Z. Deformers that move points (move, inflate, smooth, …) only
//!   move them along the chosen axes.
//! - Sizes are relative to the bounding box of the points that can move, so
//!   a deformer does the same thing to a small piece and a big one.
//! - A point with weight `w` moves `w` of the way; masked (`w = 0`) and
//!   hidden points stay where they are.

use glam::Vec3;
use rayon::prelude::*;

/// The deformers, in the order the Deform window lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Deformer {
    // Shape
    Move,
    Rotate,
    Scale,
    Bend,
    SBend,
    Skew,
    SSkew,
    Flatten,
    FlattenToAxis,
    FlattenTop,
    Twist,
    Taper,
    Squeeze,
    Perspective,
    Sag,
    // Surface
    Noise,
    Inflate,
    InflateKeepSize,
    Spherize,
    // Smoothing
    Smooth,
    Polish,
    PolishKeepDetail,
    PolishKeepGroupEdges,
    PolishKeepSharpEdges,
    Relax,
}

/// Which section of the Deform window a deformer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Shape,
    Surface,
    Smoothing,
}

impl Deformer {
    pub const ALL: [Deformer; 25] = [
        Deformer::Move,
        Deformer::Rotate,
        Deformer::Scale,
        Deformer::Bend,
        Deformer::SBend,
        Deformer::Skew,
        Deformer::SSkew,
        Deformer::Flatten,
        Deformer::FlattenToAxis,
        Deformer::FlattenTop,
        Deformer::Twist,
        Deformer::Taper,
        Deformer::Squeeze,
        Deformer::Perspective,
        Deformer::Sag,
        Deformer::Noise,
        Deformer::Inflate,
        Deformer::InflateKeepSize,
        Deformer::Spherize,
        Deformer::Smooth,
        Deformer::Polish,
        Deformer::PolishKeepDetail,
        Deformer::PolishKeepGroupEdges,
        Deformer::PolishKeepSharpEdges,
        Deformer::Relax,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Deformer::Move => "Move",
            Deformer::Rotate => "Rotate",
            Deformer::Scale => "Scale",
            Deformer::Bend => "Bend",
            Deformer::SBend => "S-bend",
            Deformer::Skew => "Skew",
            Deformer::SSkew => "S-skew",
            Deformer::Flatten => "Flatten",
            Deformer::FlattenToAxis => "Flatten to axis",
            Deformer::FlattenTop => "Flatten one side",
            Deformer::Twist => "Twist",
            Deformer::Taper => "Taper",
            Deformer::Squeeze => "Squeeze",
            Deformer::Perspective => "Perspective",
            Deformer::Sag => "Sag",
            Deformer::Noise => "Noise",
            Deformer::Inflate => "Inflate",
            Deformer::InflateKeepSize => "Inflate, keep size",
            Deformer::Spherize => "Spherize",
            Deformer::Smooth => "Smooth",
            Deformer::Polish => "Polish",
            Deformer::PolishKeepDetail => "Polish, keep detail",
            Deformer::PolishKeepGroupEdges => "Polish, keep group edges",
            Deformer::PolishKeepSharpEdges => "Polish, keep sharp edges",
            Deformer::Relax => "Relax",
        }
    }

    /// A short explanation for the tooltip.
    pub fn help(self) -> &'static str {
        match self {
            Deformer::Move => "Move the piece along the chosen axes.",
            Deformer::Rotate => "Turn the piece around the chosen axes, through the middle of its bounding box.",
            Deformer::Scale => "Make the piece bigger or smaller along the chosen axes.",
            Deformer::Bend => "Curve the piece along the chosen axis, both ends bending the same way.",
            Deformer::SBend => "Curve the piece into an S along the chosen axis: the two halves bend opposite ways.",
            Deformer::Skew => "Slant the piece along the chosen axis, like pushing over a deck of cards.",
            Deformer::SSkew => {
                "Slant the piece into an S along the chosen axis: the middle moves one way, the ends stay."
            }
            Deformer::Flatten => {
                "Squash the piece toward its middle plane across the chosen axes. Negative stretches it."
            }
            Deformer::FlattenToAxis => {
                "Squash the piece toward the line through its middle along the chosen axis, making it thinner all round."
            }
            Deformer::FlattenTop => {
                "Cut the piece flat on one side of the chosen axis: positive flattens the + side, negative the - side."
            }
            Deformer::Twist => "Twist the piece around the chosen axis, the ends turning opposite ways.",
            Deformer::Taper => "Make one end of the piece wider and the other narrower, along the chosen axis.",
            Deformer::Squeeze => "Pinch the middle of the piece in (or bulge it out) along the chosen axis.",
            Deformer::Perspective => {
                "Grow the + end of the piece along the chosen axis and keep the - end, like a view in perspective."
            }
            Deformer::Sag => {
                "Let the piece droop toward - along the chosen axis: the farther up a point is, the more it drops."
            }
            Deformer::Noise => "Roughen the surface with an even, random bumpiness. Negative dents instead of bumps.",
            Deformer::Inflate => "Push the surface out along its normals. Negative pulls it in.",
            Deformer::InflateKeepSize => {
                "Inflate the surface, then scale it back to its old size: rounds it out without growing it."
            }
            Deformer::Spherize => "Pull the piece toward a ball. Negative pushes it away from one.",
            Deformer::Smooth => "Smooth the surface; strong smoothing also shrinks it.",
            Deformer::Polish => "Smooth the surface while keeping its volume, so it doesn't shrink.",
            Deformer::PolishKeepDetail => {
                "Polish flat and gently curved areas, but leave creases, ridges and pores alone."
            }
            Deformer::PolishKeepGroupEdges => "Polish the surface, keeping the lines where face groups meet crisp.",
            Deformer::PolishKeepSharpEdges => "Polish the surface, keeping sharp edges and corners crisp.",
            Deformer::Relax => "Even out the spacing of the points without changing the shape.",
        }
    }

    pub fn section(self) -> Section {
        use Deformer::*;
        match self {
            Noise | Inflate | InflateKeepSize | Spherize => Section::Surface,
            Smooth | Polish | PolishKeepDetail | PolishKeepGroupEdges | PolishKeepSharpEdges | Relax => {
                Section::Smoothing
            }
            _ => Section::Shape,
        }
    }

    /// The axes a deformer starts with.
    pub fn default_axes(self) -> [bool; 3] {
        use Deformer::*;
        match self {
            Scale | Flatten | Noise | Inflate | InflateKeepSize | Spherize | Smooth | Polish | PolishKeepDetail
            | PolishKeepGroupEdges | PolishKeepSharpEdges | Relax => [true; 3],
            _ => [false, true, false],
        }
    }

    /// A stable key for settings files.
    pub fn key(self) -> &'static str {
        match self {
            Deformer::Move => "move",
            Deformer::Rotate => "rotate",
            Deformer::Scale => "scale",
            Deformer::Bend => "bend",
            Deformer::SBend => "s_bend",
            Deformer::Skew => "skew",
            Deformer::SSkew => "s_skew",
            Deformer::Flatten => "flatten",
            Deformer::FlattenToAxis => "flatten_to_axis",
            Deformer::FlattenTop => "flatten_one_side",
            Deformer::Twist => "twist",
            Deformer::Taper => "taper",
            Deformer::Squeeze => "squeeze",
            Deformer::Perspective => "perspective",
            Deformer::Sag => "sag",
            Deformer::Noise => "noise",
            Deformer::Inflate => "inflate",
            Deformer::InflateKeepSize => "inflate_keep_size",
            Deformer::Spherize => "spherize",
            Deformer::Smooth => "smooth",
            Deformer::Polish => "polish",
            Deformer::PolishKeepDetail => "polish_keep_detail",
            Deformer::PolishKeepGroupEdges => "polish_keep_group_edges",
            Deformer::PolishKeepSharpEdges => "polish_keep_sharp_edges",
            Deformer::Relax => "relax",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == k)
    }

    /// Smoothing-type deformers repeat a step; their cost grows with the amount.
    pub fn is_iterative(self) -> bool {
        self.section() == Section::Smoothing
    }
}

/// A piece, ready to be deformed any number of times from the same start.
pub struct Deformation {
    original: Vec<Vec3>,
    normals: Vec<Vec3>,
    weights: Vec<f32>,
    /// Neighbours of each point (compressed: `adj[off[v]..off[v + 1]]`).
    off: Vec<u32>,
    adj: Vec<u32>,
    /// Points on a face group border, for "keep group edges".
    group_edge: Vec<bool>,
    /// How strongly each point is a crease or ridge (0 to 1), for "keep detail".
    detail: Vec<f32>,
    /// Bounding box of the points that can move.
    lo: Vec3,
    hi: Vec3,
}

impl Deformation {
    /// Prepare `mesh` for deforming, with `weights[v]` (0 to 1) saying how
    /// much each point may move; usually `1 - mask`, and 0 for hidden points.
    /// Returns `None` when nothing can move. The curvature that "Polish,
    /// keep detail" needs is only worked out when `kind` is that deformer.
    pub fn new(mesh: &crate::Mesh, weights: Vec<f32>, kind: Deformer) -> Option<Self> {
        let n = mesh.positions.len();
        assert_eq!(weights.len(), n);
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (p, &w) in mesh.positions.iter().zip(&weights) {
            if w > 0.0 {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        if lo.x > hi.x {
            return None;
        }
        // Neighbours (and group edges) only matter to the smoothing deformers.
        let (off, adj, group_edge) = if kind.is_iterative() {
            let (off, adj) = neighbours(mesh);
            let group_edge = if kind == Deformer::PolishKeepGroupEdges { group_edges(mesh) } else { vec![false; n] };
            (off, adj, group_edge)
        } else {
            (vec![0; n + 1], Vec::new(), vec![false; n])
        };
        let detail = if kind == Deformer::PolishKeepDetail {
            let cav = mesh.curvature(true);
            let peak = mesh.curvature(false);
            cav.iter().zip(&peak).map(|(a, b)| a.max(*b).clamp(0.0, 1.0)).collect()
        } else {
            vec![0.0; n]
        };
        Some(Self {
            original: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            weights,
            off,
            adj,
            group_edge,
            detail,
            lo,
            hi,
        })
    }

    /// The positions the piece started from.
    pub fn original(&self) -> &[Vec3] {
        &self.original
    }

    /// The per-point weights the deformation was prepared with.
    pub fn weights(&self) -> &[f32] {
        &self.weights
    }

    fn neighbors(&self, v: usize) -> &[u32] {
        &self.adj[self.off[v] as usize..self.off[v + 1] as usize]
    }

    /// New positions for `kind` on `axes` at `amount` (-1 to 1), worked out
    /// from the original points. Points that can't move keep their place.
    pub fn apply(&self, kind: Deformer, axes: [bool; 3], amount: f32) -> Vec<Vec3> {
        let amount = amount.clamp(-1.0, 1.0);
        if amount == 0.0 || !axes.iter().any(|&a| a) {
            return self.original.clone();
        }
        let out = match kind.section() {
            Section::Smoothing => self.smoothing(kind, axes, amount),
            _ => {
                let mut pts = self.original.clone();
                self.shape(kind, axes, amount, &mut pts);
                pts
            }
        };
        // Blend by weight, and keep only the chosen axes for deformers that
        // move points rather than work along an axis.
        let along = works_along_axis(kind);
        out.into_par_iter()
            .zip(self.original.par_iter())
            .zip(self.weights.par_iter())
            .map(|((p, &o), &w)| {
                if w <= 0.0 {
                    return o;
                }
                let mut d = p - o;
                if !along {
                    for (a, &on) in axes.iter().enumerate() {
                        if !on {
                            d[a] = 0.0;
                        }
                    }
                }
                let r = o + d * w;
                if r.is_finite() { r } else { o }
            })
            .collect()
    }

    fn shape(&self, kind: Deformer, axes: [bool; 3], amount: f32, pts: &mut [Vec3]) {
        let c = (self.lo + self.hi) * 0.5;
        let h = ((self.hi - self.lo) * 0.5).max(Vec3::splat(1e-6));
        let size = h.max_element();
        let normals = &self.normals;
        match kind {
            Deformer::Move => {
                let d = Vec3::splat(amount * size);
                pts.par_iter_mut().for_each(|p| *p += d);
            }
            Deformer::Scale => {
                let f = (1.0 + amount).max(0.01);
                pts.par_iter_mut().for_each(|p| *p = c + (*p - c) * f);
            }
            Deformer::Flatten => {
                let f = 1.0 - amount;
                pts.par_iter_mut().for_each(|p| *p = c + (*p - c) * f);
            }
            Deformer::Inflate | Deformer::InflateKeepSize => {
                let d = amount * size * 0.15;
                pts.par_iter_mut().zip(normals.par_iter()).for_each(|(p, n)| *p += *n * d);
                if kind == Deformer::InflateKeepSize {
                    self.restore_size(axes, pts);
                }
            }
            Deformer::Spherize => {
                let (sum, count) = self
                    .original
                    .iter()
                    .zip(&self.weights)
                    .filter(|(_, w)| **w > 0.0)
                    .fold((0.0f32, 0usize), |(s, n), (p, _)| (s + p.distance(c), n + 1));
                let r = sum / count.max(1) as f32;
                pts.par_iter_mut().for_each(|p| {
                    let d = *p - c;
                    let len = d.length();
                    if len > 1e-9 {
                        *p += (d / len * r - d) * amount;
                    }
                });
            }
            Deformer::Noise => {
                let freq = 4.0 / size;
                let d = amount * size * 0.04;
                pts.par_iter_mut().zip(normals.par_iter()).for_each(|(p, n)| *p += *n * (value_noise(*p * freq) * d));
            }
            _ => {
                // Deformers along an axis: once per chosen axis.
                for (a, _) in axes.iter().enumerate().filter(|(_, on)| **on) {
                    let (b, cc) = ((a + 1) % 3, (a + 2) % 3);
                    along_axis(kind, a, b, cc, c, h, size, amount, pts);
                }
            }
        }
    }

    /// Scale `pts` about the middle of the movable points so their bounding
    /// box has the same size as before, on the chosen axes.
    fn restore_size(&self, axes: [bool; 3], pts: &mut [Vec3]) {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (p, &w) in pts.iter().zip(&self.weights) {
            if w > 0.0 {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        let (c0, c1) = ((self.lo + self.hi) * 0.5, (lo + hi) * 0.5);
        let f = (self.hi - self.lo) / (hi - lo).max(Vec3::splat(1e-9));
        pts.par_iter_mut().for_each(|p| {
            for a in 0..3 {
                if axes[a] {
                    p[a] = c0[a] + (p[a] - c1[a]) * f[a];
                }
            }
        });
    }

    /// Smoothing-type deformers: a number of small steps that grows with the
    /// amount, each blended by weight so masked points hold their neighbours.
    fn smoothing(&self, kind: Deformer, axes: [bool; 3], amount: f32) -> Vec<Vec3> {
        let steps = (amount.abs() * 30.0).ceil() as usize;
        let mut pts = self.original.clone();
        let mut next = pts.clone();
        // Taubin's two-step (shrink, then grow) keeps the volume (Taubin,
        // "A signal processing approach to fair surface design", SIGGRAPH 1995).
        let taubin = !matches!(kind, Deformer::Smooth | Deformer::Relax);
        let passes: &[f32] = if taubin { &[0.5, -0.53] } else { &[0.5] };
        for _ in 0..steps {
            for &lambda in passes {
                next.par_iter_mut().enumerate().for_each(|(v, out)| {
                    let p = pts[v];
                    let w = self.weights[v];
                    let ns = self.neighbors(v);
                    if w <= 0.0 || ns.is_empty() {
                        *out = p;
                        return;
                    }
                    let mut sum = Vec3::ZERO;
                    let mut total = 0.0f32;
                    for &u in ns {
                        let u = u as usize;
                        let k = match kind {
                            // Points on a group edge only follow their edge.
                            Deformer::PolishKeepGroupEdges if self.group_edge[v] => {
                                if self.group_edge[u] {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            // Neighbours across a sharp edge count for little.
                            Deformer::PolishKeepSharpEdges => self.normals[v].dot(self.normals[u]).max(0.0).powi(8),
                            _ => 1.0,
                        };
                        sum += pts[u] * k;
                        total += k;
                    }
                    if total <= 1e-9 {
                        *out = p;
                        return;
                    }
                    let mut d = sum / total - p;
                    if kind == Deformer::Relax {
                        // Slide along the surface only.
                        let n = self.normals[v];
                        d -= n * d.dot(n);
                    }
                    let keep = 1.0 - self.detail[v];
                    for a in 0..3 {
                        if !axes[a] {
                            d[a] = 0.0;
                        }
                    }
                    *out = p + d * (lambda * w * keep);
                });
                std::mem::swap(&mut pts, &mut next);
            }
        }
        pts
    }
}

/// Each point's neighbours, compressed: point `v`'s are `adj[off[v]..off[v + 1]]`.
fn neighbours(mesh: &crate::Mesh) -> (Vec<u32>, Vec<u32>) {
    let n = mesh.positions.len();
    let mut pairs: Vec<u64> = mesh
        .triangles
        .par_iter()
        .flat_map_iter(|t| {
            (0..3).flat_map(move |i| {
                let (a, b) = (t[i] as u64, t[(i + 1) % 3] as u64);
                [(a << 32) | b, (b << 32) | a]
            })
        })
        .collect();
    pairs.par_sort_unstable();
    pairs.dedup();
    let mut off = vec![0u32; n + 1];
    for &p in &pairs {
        off[(p >> 32) as usize + 1] += 1;
    }
    for v in 0..n {
        off[v + 1] += off[v];
    }
    (off, pairs.iter().map(|&p| p as u32).collect())
}

/// Which points sit where face groups meet.
fn group_edges(mesh: &crate::Mesh) -> Vec<bool> {
    let n = mesh.positions.len();
    let groups = mesh.groups();
    let mut seen = vec![u32::MAX; n];
    let mut edge = vec![false; n];
    for (t, tri) in mesh.triangles.iter().enumerate() {
        for &v in tri {
            let s = &mut seen[v as usize];
            if *s == u32::MAX {
                *s = groups[t];
            } else if *s != groups[t] {
                edge[v as usize] = true;
            }
        }
    }
    edge
}

/// Whether `kind` works along an axis (applied once per chosen axis), as
/// opposed to moving points only along the chosen axes.
fn works_along_axis(kind: Deformer) -> bool {
    use Deformer::*;
    matches!(
        kind,
        Rotate | Bend | SBend | Skew | SSkew | FlattenToAxis | FlattenTop | Twist | Taper | Squeeze | Perspective | Sag
    )
}

/// One deformer along axis `a`, curving or slanting toward axis `b` (and
/// using `cc` as the third). `c` is the middle of the bounding box and `h`
/// its half size.
#[allow(clippy::too_many_arguments)]
fn along_axis(
    kind: Deformer,
    a: usize,
    b: usize,
    cc: usize,
    c: Vec3,
    h: Vec3,
    size: f32,
    amount: f32,
    pts: &mut [Vec3],
) {
    let pi = std::f32::consts::PI;
    pts.par_iter_mut().for_each(|p| {
        let u = (p[a] - c[a]) / h[a];
        let (x, y, z) = (p[a] - c[a], p[b] - c[b], p[cc] - c[cc]);
        match kind {
            Deformer::Rotate => {
                // Around axis a: turn the (b, cc) plane.
                let (s, co) = (amount * pi).sin_cos();
                p[b] = c[b] + y * co - z * s;
                p[cc] = c[cc] + y * s + z * co;
            }
            Deformer::Bend | Deformer::SBend => {
                // A circular bend in the (a, b) plane; the S-bend flips the
                // curvature on the - half.
                let mut k = amount * pi / h[a];
                if kind == Deformer::SBend && x < 0.0 {
                    k = -k;
                }
                if k.abs() > 1e-6 {
                    let r = 1.0 / k;
                    let ang = k * x;
                    p[a] = c[a] + ang.sin() * (r - y);
                    p[b] = c[b] + r - ang.cos() * (r - y);
                }
            }
            Deformer::Skew => p[b] += amount * x,
            Deformer::SSkew => p[b] += amount * h[a] * 0.5 * (pi * u.clamp(-1.0, 1.0)).sin(),
            Deformer::FlattenToAxis => {
                let f = 1.0 - amount;
                p[b] = c[b] + y * f;
                p[cc] = c[cc] + z * f;
            }
            Deformer::FlattenTop => {
                let span = 2.0 * h[a] * amount.abs();
                if amount > 0.0 {
                    p[a] = p[a].min(c[a] + h[a] - span);
                } else {
                    p[a] = p[a].max(c[a] - h[a] + span);
                }
            }
            Deformer::Twist => {
                let (s, co) = (amount * pi * u).sin_cos();
                p[b] = c[b] + y * co - z * s;
                p[cc] = c[cc] + y * s + z * co;
            }
            Deformer::Taper => {
                let f = (1.0 + amount * u).max(0.0);
                p[b] = c[b] + y * f;
                p[cc] = c[cc] + z * f;
            }
            Deformer::Squeeze => {
                let t = u.clamp(-1.0, 1.0);
                let f = (1.0 - amount * (1.0 - t * t)).max(0.0);
                p[b] = c[b] + y * f;
                p[cc] = c[cc] + z * f;
            }
            Deformer::Perspective => {
                let t = ((u + 1.0) * 0.5).clamp(0.0, 1.0);
                let f = (1.0 + amount * t).max(0.0);
                p[b] = c[b] + y * f;
                p[cc] = c[cc] + z * f;
            }
            Deformer::Sag => {
                let t = ((u + 1.0) * 0.5).clamp(0.0, 1.0);
                p[a] -= amount * size * 0.5 * t * t;
            }
            _ => {}
        }
    });
}

/// Smooth 3D value noise in -1 to 1, from a hashed lattice.
fn value_noise(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let s = f * f * (Vec3::splat(3.0) - f * 2.0);
    let corner = |dx: f32, dy: f32, dz: f32| {
        let q = (i + Vec3::new(dx, dy, dz)).as_ivec3();
        let mut h = (q.x as u32).wrapping_mul(0x8da6_b343)
            ^ (q.y as u32).wrapping_mul(0xd816_3841)
            ^ (q.z as u32).wrapping_mul(0xcb1a_b31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 32767.5 - 1.0
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), s.x);
    let x10 = lerp(corner(0.0, 1.0, 0.0), corner(1.0, 1.0, 0.0), s.x);
    let x01 = lerp(corner(0.0, 0.0, 1.0), corner(1.0, 0.0, 1.0), s.x);
    let x11 = lerp(corner(0.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), s.x);
    lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z)
}

/// Fit `positions` (the points with `weights > 0`) into a standard size:
/// centred on the origin, the longest side of the bounding box 2 units, the
/// same proportions. Returns the new positions; `None` when nothing moves.
pub fn fit_to_standard_size(positions: &[Vec3], weights: &[f32]) -> Option<Vec<Vec3>> {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (p, &w) in positions.iter().zip(weights) {
        if w > 0.0 {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    if lo.x > hi.x {
        return None;
    }
    let c = (lo + hi) * 0.5;
    let f = 2.0 / (hi - lo).max_element().max(1e-9);
    Some(positions.iter().zip(weights).map(|(p, &w)| if w > 0.0 { (*p - c) * f } else { *p }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mesh, primitives};

    /// Deform all of `m` with `kind`, prepared as the app does.
    fn run(m: &Mesh, kind: Deformer, axes: [bool; 3], amount: f32) -> Vec<Vec3> {
        Deformation::new(m, vec![1.0; m.vertex_count()], kind).unwrap().apply(kind, axes, amount)
    }

    fn bounds(ps: &[Vec3]) -> (Vec3, Vec3) {
        ps.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }

    /// A long box along Y, for the deformers that work along an axis.
    fn bar() -> Mesh {
        let mut m = primitives::cube(8, 1.0);
        for p in &mut m.positions {
            p.y *= 4.0;
        }
        m.refresh_all();
        m
    }

    #[test]
    fn zero_amount_or_no_axes_change_nothing() {
        let m = primitives::icosphere(2, 1.0);
        for kind in Deformer::ALL {
            assert_eq!(run(&m, kind, [true; 3], 0.0), m.positions, "{kind:?}");
            assert_eq!(run(&m, kind, [false; 3], 0.7), m.positions, "{kind:?}");
        }
    }

    #[test]
    fn every_deformer_moves_something_and_stays_finite() {
        let m = bar();
        for kind in Deformer::ALL {
            for amount in [-0.6, 0.6] {
                let p = run(&m, kind, [true; 3], amount);
                assert!(p.iter().all(|v| v.is_finite()), "{kind:?} {amount}");
                assert!(p != m.positions, "{kind:?} {amount} moved nothing");
            }
        }
    }

    #[test]
    fn masked_points_stay_put() {
        let m = bar();
        let weights: Vec<f32> = m.positions.iter().map(|p| if p.y < 0.0 { 0.0 } else { 1.0 }).collect();
        for kind in Deformer::ALL {
            let p = Deformation::new(&m, weights.clone(), kind).unwrap().apply(kind, [true; 3], 0.8);
            for v in 0..p.len() {
                if weights[v] == 0.0 {
                    assert_eq!(p[v], m.positions[v], "{kind:?}");
                }
            }
        }
        assert!(Deformation::new(&m, vec![0.0; m.vertex_count()], Deformer::Move).is_none(), "nothing can move");
    }

    #[test]
    fn shape_deformers_do_what_they_say() {
        let m = bar();
        let (lo, hi) = bounds(&m.positions);
        // Move along Y only.
        let p = run(&m, Deformer::Move, [false, true, false], 0.5);
        assert!((p[0] - m.positions[0]).x.abs() < 1e-6 && (p[0] - m.positions[0]).y > 0.0);
        // Scale doubles the size on the chosen axis.
        let (l, h) = bounds(&run(&m, Deformer::Scale, [true, false, false], 1.0));
        assert!(((h.x - l.x) - 2.0 * (hi.x - lo.x)).abs() < 1e-3);
        assert!(((h.y - l.y) - (hi.y - lo.y)).abs() < 1e-3);
        // Flatten fully: flat along Y.
        let (l, h) = bounds(&run(&m, Deformer::Flatten, [false, true, false], 1.0));
        assert!(h.y - l.y < 1e-4);
        // Rotate half a turn around Y swaps +X and -X.
        let p = run(&m, Deformer::Rotate, [false, true, false], 1.0);
        let far = (0..p.len()).max_by(|&a, &b| m.positions[a].x.total_cmp(&m.positions[b].x)).unwrap();
        assert!(p[far].x < -0.4, "{}", p[far]);
        // Bend along Y: the ends move toward +Z (the bar's middle stays).
        let p = run(&m, Deformer::Bend, [false, true, false], 0.5);
        let top = (0..p.len()).max_by(|&a, &b| m.positions[a].y.total_cmp(&m.positions[b].y)).unwrap();
        let bottom = (0..p.len()).min_by(|&a, &b| m.positions[a].y.total_cmp(&m.positions[b].y)).unwrap();
        assert!(p[top].z - m.positions[top].z > 0.5 && p[bottom].z - m.positions[bottom].z > 0.5);
        // S-bend: the ends go opposite ways.
        let p = run(&m, Deformer::SBend, [false, true, false], 0.5);
        assert!((p[top].z - m.positions[top].z) * (p[bottom].z - m.positions[bottom].z) < 0.0);
        // Taper: the + end widens, the - end narrows.
        let p = run(&m, Deformer::Taper, [false, true, false], 0.5);
        assert!(
            p[top].x.abs() > m.positions[top].x.abs() - 1e-6 && p[bottom].x.abs() < m.positions[bottom].x.abs() + 1e-6
        );
        // Flatten one side at 0.25 cuts the + end flat a quarter of the height down.
        let (l, h) = bounds(&run(&m, Deformer::FlattenTop, [false, true, false], 0.25));
        assert!((h.y - (hi.y - 0.25 * (hi.y - lo.y))).abs() < 1e-4, "{} {}", h.y, hi.y);
        assert!((l.y - lo.y).abs() < 1e-6, "the other end stays");
        // Sag drops the top and leaves the bottom.
        let p = run(&m, Deformer::Sag, [false, true, false], 0.5);
        assert!(p[top].y < m.positions[top].y && (p[bottom].y - m.positions[bottom].y).abs() < 1e-5);
    }

    #[test]
    fn inflate_keep_size_keeps_the_bounding_box() {
        let m = primitives::cube(6, 1.0);
        let (lo, hi) = bounds(&m.positions);
        let p = run(&m, Deformer::InflateKeepSize, [true; 3], 0.8);
        let (l, h) = bounds(&p);
        assert!((h - l - (hi - lo)).abs().max_element() < 1e-3, "{l} {h}");
        assert!(p != m.positions, "it still changes the shape");
    }

    #[test]
    fn polish_keeps_volume_better_than_smooth() {
        let mut m = primitives::icosphere(3, 1.0);
        // Rough it up so there's something to smooth.
        for (i, p) in m.positions.iter_mut().enumerate() {
            *p *= 1.0 + 0.05 * ((i * 7919 % 13) as f32 / 13.0 - 0.5);
        }
        m.refresh_all();
        let radius = |ps: &[Vec3]| ps.iter().map(|p| p.length()).sum::<f32>() / ps.len() as f32;
        let r0 = radius(&m.positions);
        let smooth = radius(&run(&m, Deformer::Smooth, [true; 3], 1.0));
        let polish = radius(&run(&m, Deformer::Polish, [true; 3], 1.0));
        assert!(smooth < r0 && (r0 - polish).abs() < (r0 - smooth).abs(), "{r0} {smooth} {polish}");
        // Relax barely changes the shape.
        let relaxed = radius(&run(&m, Deformer::Relax, [true; 3], 1.0));
        assert!((relaxed - r0).abs() < (r0 - smooth).abs());
    }

    #[test]
    fn polish_keep_group_edges_keeps_the_edge_line() {
        let mut m = primitives::icosphere(3, 1.0);
        for (i, p) in m.positions.iter_mut().enumerate() {
            *p *= 1.0 + 0.08 * ((i * 104729 % 17) as f32 / 17.0 - 0.5);
        }
        m.refresh_all();
        let groups = m.triangles.iter().map(|t| u32::from(m.positions[t[0] as usize].x > 0.0)).collect();
        m.set_groups(groups);
        let plain = run(&m, Deformer::Polish, [true; 3], 1.0);
        let kept = run(&m, Deformer::PolishKeepGroupEdges, [true; 3], 1.0);
        // Edge points move only along their edge, so they move differently.
        let edge: Vec<usize> = (0..m.vertex_count()).filter(|&v| group_edges(&m)[v]).collect();
        assert!(!edge.is_empty());
        assert!(edge.iter().any(|&v| (plain[v] - kept[v]).length() > 1e-4));
    }

    #[test]
    fn fit_to_standard_size_centres_and_scales() {
        let m = bar();
        let moved: Vec<Vec3> = m.positions.iter().map(|p| *p * 3.0 + Vec3::new(5.0, 1.0, -2.0)).collect();
        let p = fit_to_standard_size(&moved, &vec![1.0; moved.len()]).unwrap();
        let (l, h) = bounds(&p);
        assert!(((l + h) * 0.5).length() < 1e-4);
        assert!(((h - l).max_element() - 2.0).abs() < 1e-4);
    }

    #[test]
    fn keys_round_trip() {
        for k in Deformer::ALL {
            assert_eq!(Deformer::from_key(k.key()), Some(k));
        }
    }
}
