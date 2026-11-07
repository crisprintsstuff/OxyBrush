//! Curve deformation ("Curve Move"): bend and stretch the mesh around a
//! curve by moving the curve's control points.
//!
//! The idea follows the wire deformer of Singh and Fiume, "Wires: A
//! Geometric Deformation Technique" (SIGGRAPH 1998): each vertex near the
//! curve is tied to its closest point on the curve as first drawn (the
//! *rest* curve), as an offset in that point's local frame. When the curve
//! moves, the vertex goes to the same offset in the frame of the matching
//! point on the moved curve, blended by a falloff on its distance to the
//! rest curve. Frames along the curve are rotation-minimizing, computed with
//! the double-reflection method of Wang, Jüttler, Zheng and Liu,
//! "Computation of Rotation Minimizing Frames" (ACM TOG 27(1), 2008), so
//! the curve doesn't twist the mesh on its own.
//!
//! Everything here is deterministic and works from the vertices' original
//! positions, so applying the same curve twice gives the same mesh.

use crate::falloff::Falloff;
use glam::{IVec3, Quat, Vec3};
use oxy_mesh::Mesh;
use rayon::prelude::*;
use std::collections::HashMap;

/// Catmull-Rom points through `points`, `per_segment` samples per segment
/// (plus the last point of an open curve). Two curves with the same number
/// of control points sampled with the same `per_segment` correspond sample
/// for sample, which is what lets a moved curve drive its rest curve.
pub fn sample_curve(points: &[Vec3], closed: bool, per_segment: usize) -> Vec<Vec3> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let closed = closed && n >= 3;
    let k = per_segment.max(1);
    let at = |i: isize| {
        if closed { points[i.rem_euclid(n as isize) as usize] } else { points[i.clamp(0, n as isize - 1) as usize] }
    };
    let segments = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(segments * k + 1);
    for seg in 0..segments {
        let (p0, p1, p2, p3) = (at(seg as isize - 1), at(seg as isize), at(seg as isize + 1), at(seg as isize + 2));
        for j in 0..k {
            let t = j as f32 / k as f32;
            out.push(catmull_rom(p0, p1, p2, p3, t));
        }
    }
    // An open curve ends on its last point; a closed one returns to its first.
    out.push(if closed { points[0] } else { points[n - 1] });
    out
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let (t2, t3) = (t * t, t * t * t);
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Rotation-minimizing frames along a polyline, as rotations taking
/// (X, Y, Z) to (tangent, normal, binormal). `start` is the first frame;
/// `None` builds one from the first tangent and `up`.
fn frames(samples: &[Vec3], up: Vec3, start: Option<Quat>) -> Vec<Quat> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    let tangent = |i: usize| {
        let (a, b) = if n == 1 {
            (samples[0], samples[0])
        } else if i == 0 {
            (samples[0], samples[1])
        } else if i == n - 1 {
            (samples[n - 2], samples[n - 1])
        } else {
            (samples[i - 1], samples[i + 1])
        };
        (b - a).normalize_or(Vec3::X)
    };
    let t0 = tangent(0);
    let first = start.unwrap_or_else(|| {
        let mut r = up - t0 * up.dot(t0);
        if r.length_squared() < 1e-8 {
            r = t0.any_orthonormal_vector();
        }
        let r = r.normalize();
        Quat::from_mat3(&glam::Mat3::from_cols(t0, r, t0.cross(r)))
    });
    let mut out = Vec::with_capacity(n);
    out.push(first);
    let mut r = first * Vec3::Y;
    let mut t = t0;
    for i in 1..n {
        // Double reflection (Wang et al. 2008, algorithm in section 4).
        let ti = tangent(i);
        let v1 = samples[i] - samples[i - 1];
        let c1 = v1.length_squared();
        let (r_l, t_l) =
            if c1 > 1e-20 { (r - (2.0 / c1) * v1.dot(r) * v1, t - (2.0 / c1) * v1.dot(t) * v1) } else { (r, t) };
        let v2 = ti - t_l;
        let c2 = v2.length_squared();
        let ri = if c2 > 1e-20 { r_l - (2.0 / c2) * v2.dot(r_l) * v2 } else { r_l };
        let ri = (ri - ti * ri.dot(ti)).normalize_or(ti.any_orthonormal_vector());
        out.push(Quat::from_mat3(&glam::Mat3::from_cols(ti, ri, ti.cross(ri))).normalize());
        r = ri;
        t = ti;
    }
    out
}

/// A curve as the deformer sees it: sample points and their frames.
#[derive(Debug, Clone)]
pub struct WireCurve {
    pub samples: Vec<Vec3>,
    pub frames: Vec<Quat>,
}

impl WireCurve {
    /// The rest curve: frames start from `up` (e.g. the surface normal
    /// under the first control point).
    pub fn rest(samples: Vec<Vec3>, up: Vec3) -> Self {
        let frames = frames(&samples, up, None);
        Self { samples, frames }
    }

    /// The moved curve matching `rest` sample for sample. Its first frame is
    /// the rest curve's, turned by the smallest rotation between the two
    /// first tangents, so moving the curve doesn't spin the mesh around it.
    pub fn moved(samples: Vec<Vec3>, rest: &WireCurve) -> Self {
        let start = match (rest.frames.first(), samples.len()) {
            (Some(&q), n) if n >= 2 => {
                let t_rest = q * Vec3::X;
                let t_now = (samples[1] - samples[0]).normalize_or(t_rest);
                Some((Quat::from_rotation_arc(t_rest, t_now) * q).normalize())
            }
            (q, _) => q.copied(),
        };
        let frames = frames(&samples, Vec3::Y, start);
        Self { samples, frames }
    }

    fn point(&self, seg: usize, f: f32) -> (Vec3, Quat) {
        let last = self.samples.len() - 1;
        let (a, b) = (seg.min(last), (seg + 1).min(last));
        (self.samples[a].lerp(self.samples[b], f), self.frames[a].slerp(self.frames[b], f))
    }
}

/// Share of the reach, from the curve outward, that follows the curve
/// fully; the falloff covers the rest. Without a rigid core, the surface of
/// a limb (partway out along the falloff) lags behind its centre and the
/// bent part swells and smears; with it, the limb moves like a solid part
/// and only the area around it stretches.
pub const RIGID_CORE: f32 = 0.5;

/// Settings for binding the mesh to a curve.
#[derive(Debug, Clone, Copy)]
pub struct BindSettings {
    pub falloff: Falloff,
    /// Masked vertices (mask 1) don't move.
    pub use_mask: bool,
}

impl Default for BindSettings {
    fn default() -> Self {
        Self { falloff: Falloff::Smooth, use_mask: true }
    }
}

/// Vertices tied to a rest curve (one or more symmetric copies).
#[derive(Debug, Clone, Default)]
pub struct Binding {
    /// Bound vertex indices, ascending.
    pub vertices: Vec<u32>,
    /// Chunks that own at least one bound vertex, ascending.
    pub chunks: Vec<u32>,
    /// Which curve copy each vertex follows.
    copy: Vec<u16>,
    /// Segment of the sampled curve, and position along it.
    seg: Vec<u32>,
    frac: Vec<f32>,
    /// Offset from the curve in the rest frame.
    local: Vec<Vec3>,
    /// How much the vertex follows (falloff × strength × unmasked).
    weight: Vec<f32>,
}

impl Binding {
    /// Tie the vertices of `mesh` near any of the `rest` curves (symmetric
    /// copies, all sampled alike). `radius[i]` is the reach at sample `i`;
    /// `strength[i]` scales how much vertices there follow.
    pub fn bind(mesh: &Mesh, rest: &[WireCurve], radius: &[f32], strength: &[f32], s: BindSettings) -> Self {
        let Some(first) = rest.first() else { return Self::default() };
        let n = first.samples.len();
        if n < 2 || radius.len() != n || strength.len() != n {
            return Self::default();
        }
        let reach = radius.iter().copied().fold(0.0f32, f32::max);
        if reach.is_nan() || reach <= 0.0 {
            return Self::default();
        }
        // Segments in a hash grid with cells as big as the largest reach, so
        // each vertex only looks at the few segments that could touch it.
        let cell = reach;
        let key = |p: Vec3| (p / cell).floor().as_ivec3();
        let mut grid: HashMap<IVec3, Vec<(u16, u32)>> = HashMap::new();
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for (c, curve) in rest.iter().enumerate() {
            for i in 0..n - 1 {
                let r = radius[i].max(radius[i + 1]);
                let a = curve.samples[i].min(curve.samples[i + 1]) - Vec3::splat(r);
                let b = curve.samples[i].max(curve.samples[i + 1]) + Vec3::splat(r);
                lo = lo.min(a);
                hi = hi.max(b);
                let (ka, kb) = (key(a), key(b));
                for x in ka.x..=kb.x {
                    for y in ka.y..=kb.y {
                        for z in ka.z..=kb.z {
                            grid.entry(IVec3::new(x, y, z)).or_default().push((c as u16, i as u32));
                        }
                    }
                }
            }
        }
        let chunks: Vec<usize> = mesh
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.aabb.min.cmple(hi).all() && c.aabb.max.cmpge(lo).all())
            .map(|(i, _)| i)
            .collect();
        type Bound = (u32, u16, u32, f32, Vec3, f32);
        let found: Vec<(usize, Vec<Bound>)> = chunks
            .par_iter()
            .map(|&ci| {
                let mut out = Vec::new();
                for v in mesh.chunks[ci].vertices.clone() {
                    let p = mesh.positions[v as usize];
                    let Some(cands) = grid.get(&key(p)) else { continue };
                    // Closest point over the candidate segments, measured
                    // against the reach there.
                    let mut best: Option<(f32, u16, u32, f32, f32)> = None; // (d / r, copy, seg, frac, d)
                    for &(c, i) in cands {
                        let curve = &rest[c as usize];
                        let (a, b) = (curve.samples[i as usize], curve.samples[i as usize + 1]);
                        let ab = b - a;
                        let f = if ab.length_squared() > 1e-20 {
                            ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let d = p.distance(a + ab * f);
                        let r = radius[i as usize] + (radius[i as usize + 1] - radius[i as usize]) * f;
                        if r <= 0.0 || d >= r {
                            continue;
                        }
                        let rel = d / r;
                        if best.is_none_or(|b| d < b.4 - 1e-7 || (d <= b.4 + 1e-7 && c < b.1)) {
                            best = Some((rel, c, i, f, d));
                        }
                    }
                    let Some((rel, c, i, f, _)) = best else { continue };
                    let st = strength[i as usize] + (strength[i as usize + 1] - strength[i as usize]) * f;
                    let unmasked =
                        if s.use_mask { 1.0 - mesh.mask.get(v as usize).copied().unwrap_or(0.0) } else { 1.0 };
                    let soft = ((rel - RIGID_CORE) / (1.0 - RIGID_CORE)).max(0.0);
                    let w = s.falloff.eval(soft) * st.clamp(0.0, 1.0) * unmasked.clamp(0.0, 1.0);
                    if w <= 0.0 {
                        continue;
                    }
                    let (q, frame) = rest[c as usize].point(i as usize, f);
                    out.push((v, c, i, f, frame.inverse() * (p - q), w));
                }
                (ci, out)
            })
            .collect();
        let mut b = Self::default();
        for (ci, list) in found {
            if !list.is_empty() {
                b.chunks.push(ci as u32);
            }
            for (v, c, i, f, local, w) in list {
                b.vertices.push(v);
                b.copy.push(c);
                b.seg.push(i);
                b.frac.push(f);
                b.local.push(local);
                b.weight.push(w);
            }
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// New positions of the bound vertices (in `vertices` order) for the
    /// moved curves (`moved[c]` matches the rest copy `c`), from their
    /// `original` positions.
    pub fn deform(&self, original: &[Vec3], moved: &[WireCurve]) -> Vec<Vec3> {
        (0..self.vertices.len())
            .into_par_iter()
            .map(|k| {
                let v = self.vertices[k] as usize;
                let Some(curve) = moved.get(self.copy[k] as usize) else { return original[v] };
                let (q, frame) = curve.point(self.seg[k] as usize, self.frac[k]);
                let target = q + frame * self.local[k];
                original[v].lerp(target, self.weight[k])
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn line(n: usize) -> Vec<Vec3> {
        (0..n).map(|i| Vec3::new(0.0, -1.0 + 2.0 * i as f32 / (n - 1) as f32, 0.0)).collect()
    }

    #[test]
    fn sampling_is_correspondent_and_ends_on_the_points() {
        let pts = line(3);
        let s = sample_curve(&pts, false, 8);
        assert_eq!(s.len(), 2 * 8 + 1);
        assert_eq!(s[0], pts[0]);
        assert_eq!(*s.last().unwrap(), pts[2]);
        assert!((s[8] - pts[1]).length() < 1e-6);
        let closed = sample_curve(&[Vec3::X, Vec3::Y, Vec3::Z], true, 4);
        assert_eq!(closed.len(), 3 * 4 + 1);
        assert_eq!(closed[0], *closed.last().unwrap());
    }

    #[test]
    fn rotation_minimizing_frames_follow_a_bend_without_twist() {
        // A quarter circle in the XY plane: the binormal stays along Z.
        let s: Vec<Vec3> = (0..=32)
            .map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::FRAC_PI_2;
                Vec3::new(a.cos(), a.sin(), 0.0)
            })
            .collect();
        let c = WireCurve::rest(s, Vec3::Z.cross(Vec3::Y));
        for q in &c.frames {
            assert!((q * Vec3::Z).dot(Vec3::Z).abs() > 0.999, "{:?}", q * Vec3::Z);
        }
    }

    fn cylinder() -> Mesh {
        // A unit sphere stretched into a capsule-like rod along Y.
        let mut d = primitives::icosphere(4, 1.0).to_data();
        for p in &mut d.positions {
            *p = Vec3::new(p.x * 0.3, p.y * 2.0, p.z * 0.3);
        }
        Mesh::from_data(d)
    }

    #[test]
    fn unmoved_curve_leaves_the_mesh_alone() {
        let mesh = cylinder();
        let rest = WireCurve::rest(sample_curve(&line(4), false, 8), Vec3::Z);
        let n = rest.samples.len();
        let b =
            Binding::bind(&mesh, std::slice::from_ref(&rest), &vec![0.6; n], &vec![1.0; n], BindSettings::default());
        assert!(!b.is_empty());
        let moved = WireCurve::moved(rest.samples.clone(), &rest);
        let out = b.deform(&mesh.positions, &[moved]);
        for (k, &v) in b.vertices.iter().enumerate() {
            assert!((out[k] - mesh.positions[v as usize]).length() < 1e-4);
        }
    }

    #[test]
    fn moving_the_curve_moves_and_bends_the_mesh() {
        let mesh = cylinder();
        let pts = line(3);
        let rest = WireCurve::rest(sample_curve(&pts, false, 16), Vec3::Z);
        let n = rest.samples.len();
        let b =
            Binding::bind(&mesh, std::slice::from_ref(&rest), &vec![0.6; n], &vec![1.0; n], BindSettings::default());
        // Translate the whole curve: everything bound moves with it, by up to
        // the shift (scaled by its falloff weight).
        let shift = Vec3::new(0.5, 0.0, 0.0);
        let moved = WireCurve::moved(rest.samples.iter().map(|p| *p + shift).collect(), &rest);
        let out = b.deform(&mesh.positions, &[moved]);
        for (k, &v) in b.vertices.iter().enumerate() {
            let d = out[k] - mesh.positions[v as usize];
            assert!(d.y.abs() < 1e-3 && d.z.abs() < 1e-3 && (-1e-4..=0.5001).contains(&d.x), "{d}");
        }
        // Bend the tip sideways: the top moves, the bottom stays.
        let mut bent = pts.clone();
        bent[2] = Vec3::new(1.0, 0.0, 0.0);
        let moved = WireCurve::moved(sample_curve(&bent, false, 16), &rest);
        let out = b.deform(&mesh.positions, &[moved]);
        let top = b.vertices.iter().position(|&v| mesh.positions[v as usize].y > 0.95).expect("top bound");
        let bottom = b.vertices.iter().position(|&v| mesh.positions[v as usize].y < -0.95).expect("bottom bound");
        assert!(out[top].x > 0.4, "top follows the tip: {}", out[top]);
        assert!((out[bottom] - mesh.positions[b.vertices[bottom] as usize]).length() < 0.05);
    }

    #[test]
    fn mask_and_distance_limit_what_moves() {
        let mut mesh = cylinder();
        // Mask everything with x < 0.
        let mask: Vec<f32> = mesh.positions.iter().map(|p| if p.x < 0.0 { 1.0 } else { 0.0 }).collect();
        mesh.mask = mask;
        let rest = WireCurve::rest(sample_curve(&line(3), false, 8), Vec3::Z);
        let n = rest.samples.len();
        let b =
            Binding::bind(&mesh, std::slice::from_ref(&rest), &vec![0.6; n], &vec![1.0; n], BindSettings::default());
        assert!(b.vertices.iter().all(|&v| mesh.positions[v as usize].x >= 0.0));
        // Far vertices aren't bound at all.
        let thin = Binding::bind(&mesh, &[rest], &vec![0.05; n], &vec![1.0; n], BindSettings::default());
        assert!(thin.vertices.len() < b.vertices.len());
    }

    #[test]
    fn deform_is_deterministic() {
        let mesh = cylinder();
        let rest = WireCurve::rest(sample_curve(&line(3), false, 8), Vec3::Z);
        let n = rest.samples.len();
        let b =
            Binding::bind(&mesh, std::slice::from_ref(&rest), &vec![0.6; n], &vec![1.0; n], BindSettings::default());
        let moved = WireCurve::moved(rest.samples.iter().map(|p| *p * 1.3).collect(), &rest);
        assert_eq!(b.deform(&mesh.positions, std::slice::from_ref(&moved)), b.deform(&mesh.positions, &[moved]));
    }
}
