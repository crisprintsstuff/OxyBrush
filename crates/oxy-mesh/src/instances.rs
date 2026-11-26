//! Surface instances (RFC 0022): where copies of one piece (the *source*)
//! go on another (the *host*), and how each is turned and sized.
//!
//! Placements are kept as [`Anchor`]s: a few host points and weights,
//! not positions. So when the host is sculpted, [`transforms`] puts every
//! copy back on the moved surface; only a change of the host's triangles
//! needs new anchors ([`anchors`] again).
//!
//! INVARIANT: an anchor's point indices are valid for the host it was made
//! from (same topology). Callers rebuild anchors when the topology changes.

use crate::Mesh;
use glam::{IVec3, Mat3, Mat4, Vec3};
use std::collections::HashMap;

/// Where copies go on the host.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Place {
    /// One per polygon (quads kept where the host has them).
    #[default]
    Faces,
    /// One per point.
    Points,
    /// One per polygon of one face group.
    Group(u32),
    /// One per visible polygon whose mean mask meets this threshold.
    Mask(f32),
    /// Seeded samples over the visible surface, this far apart in scene units.
    Scatter(f32),
    /// Scatter only on faces whose mean mask meets the threshold.
    ScatterMask { spacing: f32, threshold: f32 },
    /// Scatter only on polygons of one face group.
    ScatterGroup { spacing: f32, group: u32 },
}

impl Place {
    /// Mask / any scatter mode — needs the `INS2`/`INS3` section.
    pub fn extended(self) -> bool {
        matches!(self, Self::Mask(_) | Self::Scatter(_) | Self::ScatterMask { .. } | Self::ScatterGroup { .. })
    }

    /// Scatter restricted to mask or group — needs `INS3` (extra fields).
    pub fn needs_ins3(self) -> bool {
        matches!(self, Self::ScatterMask { .. } | Self::ScatterGroup { .. })
    }

    /// Spacing when this is any scatter mode.
    pub fn scatter_spacing(self) -> Option<f32> {
        match self {
            Self::Scatter(s) | Self::ScatterMask { spacing: s, .. } | Self::ScatterGroup { spacing: s, .. } => Some(s),
            _ => None,
        }
    }

    /// Mutable spacing for scatter modes (UI drag).
    pub fn scatter_spacing_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Scatter(s) | Self::ScatterMask { spacing: s, .. } | Self::ScatterGroup { spacing: s, .. } => Some(s),
            _ => None,
        }
    }
}

/// Which way a copy's forward axis (+Z of the source) points, around the
/// surface normal (its +Y).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Orient {
    /// A direction in the world, laid onto the surface: copies never flip
    /// between neighbours.
    #[default]
    World,
    /// Along the polygon's longest edge (for points: the longest edge out
    /// of the point). Follows the surface; can flip between neighbours.
    LongestEdge,
    /// Along the polygon's first edge, in its point order.
    FirstEdge,
    /// A different spin for every copy.
    Random,
    /// Host body grain: same as [`World`], but callers set [`Settings::direction`]
    /// from [`host_flow_direction`] so rows share a consistent direction on limbs.
    Flow,
}

/// Longest axis of the host's axis-aligned bounds — a stable “along the body”
/// direction for [`Orient::Flow`] (neck / tail / limb grain).
pub fn host_flow_direction(host: &Mesh) -> Vec3 {
    let e = host.bounds().extent();
    if e.x >= e.y && e.x >= e.z {
        Vec3::X
    } else if e.z >= e.y {
        Vec3::Z
    } else {
        Vec3::Y
    }
}

/// How big each copy is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeMode {
    /// `size` is the copy's largest dimension, in scene units.
    #[default]
    Absolute,
    /// `size` times the polygon's (or the point's) average edge length.
    Face,
}

/// Everything that decides where copies go and how they look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub place: Place,
    pub orient: Orient,
    /// For [`Orient::World`] and [`Orient::Flow`]: the world direction forward follows.
    pub direction: Vec3,
    pub size_mode: SizeMode,
    pub size: f32,
    /// Along the normal, as a share of the copy's size (0: the copy's base
    /// sits on the surface; negative sinks it in).
    pub offset: f32,
    /// Extra turn around the normal, degrees.
    pub spin: f32,
    /// Random size change, 0..=1 (1: from half to one and a half times).
    pub vary_size: f32,
    /// Random spin, 0..=1 (1: any angle).
    pub vary_spin: f32,
    pub seed: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            place: Place::Faces,
            orient: Orient::World,
            direction: Vec3::Z,
            size_mode: SizeMode::Face,
            size: 0.8,
            offset: 0.0,
            spin: 0.0,
            vary_size: 0.0,
            vary_spin: 0.0,
            seed: 1,
        }
    }
}

/// Where one copy sits: a weighted average of up to four host points, and
/// the point pair its forward edge runs along (the same point twice when
/// there is none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub points: [u32; 4],
    pub weights: [f32; 4],
    pub edge: [u32; 2],
    /// The anchor's own number, so random variation stays with it.
    pub key: u32,
}

/// The host's polygons: quads where two triangles share a quad diagonal,
/// otherwise triangles. Each is its points in order around it, and the
/// triangles it covers. Hidden triangles are left out.
pub fn polygons(host: &Mesh) -> Vec<(Vec<u32>, Vec<u32>)> {
    let diagonals = host.quad_diagonals_ref();
    let hidden = host.hidden();
    // Diagonal edge -> the triangles on it (two for a quad).
    let mut by_diag: HashMap<[u32; 2], Vec<u32>> = HashMap::new();
    let mut paired = vec![false; host.triangles.len()];
    for (t, tri) in host.triangles.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            let e = [a.min(b), a.max(b)];
            if diagonals.contains(&e) {
                by_diag.entry(e).or_default().push(t as u32);
            }
        }
    }
    let mut out = Vec::new();
    for (e, tris) in &by_diag {
        let &[t0, t1] = tris.as_slice() else { continue };
        if paired[t0 as usize] || paired[t1 as usize] || hidden[t0 as usize] || hidden[t1 as usize] {
            continue;
        }
        paired[t0 as usize] = true;
        paired[t1 as usize] = true;
        // Walk t0 starting after the diagonal, then add t1's far corner.
        let a = host.triangles[t0 as usize];
        let start = (0..3).find(|&k| a[k] != e[0] && a[k] != e[1]).unwrap_or(0);
        let p0 = a[start];
        let p1 = a[(start + 1) % 3];
        let p3 = a[(start + 2) % 3];
        let b = host.triangles[t1 as usize];
        let p2 = b.iter().copied().find(|&v| v != e[0] && v != e[1]).unwrap_or(p1);
        out.push((vec![p0, p1, p2, p3], vec![t0, t1]));
    }
    for (t, tri) in host.triangles.iter().enumerate() {
        if !paired[t] && !hidden[t] {
            out.push((tri.to_vec(), vec![t as u32]));
        }
    }
    // A stable order (hash maps don't keep one): by first triangle.
    out.sort_unstable_by_key(|(_, tris)| tris[0]);
    out
}

/// Candidate anchors on `host` for `settings.place`. Mask placement
/// filters the candidates in [`transforms`], using the live mask.
pub fn anchors(host: &Mesh, settings: &Settings) -> Vec<Anchor> {
    let mut out = Vec::new();
    match settings.place {
        Place::Faces | Place::Group(_) | Place::Mask(_) => {
            let groups = host.groups();
            for (pts, tris) in polygons(host) {
                if let Place::Group(g) = settings.place
                    && groups[tris[0] as usize] != g
                {
                    continue;
                }
                let n = pts.len().min(4);
                let mut points = [pts[0]; 4];
                let mut weights = [0.0; 4];
                for k in 0..n {
                    points[k] = pts[k];
                    weights[k] = 1.0 / n as f32;
                }
                let edge = match settings.orient {
                    Orient::LongestEdge => {
                        let len = |k: usize| {
                            host.positions[pts[k] as usize].distance_squared(host.positions[pts[(k + 1) % n] as usize])
                        };
                        let k = (0..n).max_by(|&a, &b| len(a).total_cmp(&len(b))).unwrap_or(0);
                        [pts[k], pts[(k + 1) % n]]
                    }
                    _ => [pts[0], pts[1]],
                };
                out.push(Anchor { points, weights, edge, key: out.len() as u32 });
            }
        }
        Place::Points => {
            for v in 0..host.vertex_count() as u32 {
                if host.is_vertex_hidden(v) || host.vertex_triangles(v).is_empty() {
                    continue;
                }
                let neighbors = host.neighbors(v);
                let p = host.positions[v as usize];
                let pick = match settings.orient {
                    Orient::LongestEdge => neighbors.iter().copied().max_by(|&a, &b| {
                        p.distance_squared(host.positions[a as usize])
                            .total_cmp(&p.distance_squared(host.positions[b as usize]))
                    }),
                    _ => neighbors.iter().copied().min(),
                };
                let edge = [v, pick.unwrap_or(v)];
                out.push(Anchor { points: [v; 4], weights: [1.0, 0.0, 0.0, 0.0], edge, key: out.len() as u32 });
            }
        }
        Place::Scatter(spacing) => {
            return scatter_anchors(
                &host.positions,
                &host.triangles,
                host.hidden(),
                ScatterOpts { spacing, seed: settings.seed, orient: settings.orient, ..Default::default() },
            );
        }
        Place::ScatterMask { spacing, threshold } => {
            return scatter_anchors(
                &host.positions,
                &host.triangles,
                host.hidden(),
                ScatterOpts {
                    spacing,
                    seed: settings.seed,
                    orient: settings.orient,
                    mask: Some((&host.mask, threshold)),
                    ..Default::default()
                },
            );
        }
        Place::ScatterGroup { spacing, group } => {
            return scatter_anchors(
                &host.positions,
                &host.triangles,
                host.hidden(),
                ScatterOpts {
                    spacing,
                    seed: settings.seed,
                    orient: settings.orient,
                    group: Some((host.groups(), group)),
                    ..Default::default()
                },
            );
        }
    }
    out
}

/// Options for [`scatter_anchors`]: spacing, seed, orient, and optional filters.
#[derive(Clone, Copy, Default)]
pub struct ScatterOpts<'a> {
    pub spacing: f32,
    pub seed: u32,
    pub orient: Orient,
    /// Vertex mask + mean threshold.
    pub mask: Option<(&'a [f32], f32)>,
    /// Per-triangle groups + wanted id.
    pub group: Option<(&'a [u32], u32)>,
}

/// A bounded, area-weighted dart throw over visible triangles. A spatial
/// grid rejects candidates closer than the requested scene-space spacing.
/// When `opts.mask` or `opts.group` is set, only matching triangles are sampled.
pub fn scatter_anchors(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    hidden: &[bool],
    opts: ScatterOpts<'_>,
) -> Vec<Anchor> {
    let ScatterOpts { spacing, seed, orient, mask, group } = opts;
    if !spacing.is_finite() || spacing <= 0.0 {
        return Vec::new();
    }
    const MAX_COPIES: usize = 20_000;
    const MAX_ATTEMPTS: u32 = 200_000;
    let mut area = 0.0_f64;
    let mut faces = Vec::new();
    for (ti, (&tri, &hidden)) in triangles.iter().zip(hidden).enumerate() {
        if hidden {
            continue;
        }
        if let Some((groups, want)) = group
            && groups.get(ti).copied().unwrap_or(0) != want
        {
            continue;
        }
        if let Some((masks, threshold)) = mask {
            let mean = tri.iter().map(|&v| masks.get(v as usize).copied().unwrap_or(0.0)).sum::<f32>() / 3.0;
            if mean < threshold {
                continue;
            }
        }
        let [a, b, c] = tri.map(|i| positions[i as usize]);
        let face_area = (b - a).cross(c - a).length() as f64 * 0.5;
        if face_area > 0.0 && face_area.is_finite() {
            area += face_area;
            faces.push((area, tri));
        }
    }
    let Some(&(_, first)) = faces.first() else { return Vec::new() };
    let origin = positions[first[0] as usize];
    let attempts = (24.0 * area / (spacing as f64).powi(2)).ceil().clamp(64.0, MAX_ATTEMPTS as f64) as u32;
    let min_distance = spacing * spacing;
    let mut grid: HashMap<IVec3, Vec<Vec3>> = HashMap::new();
    let mut anchors = Vec::new();
    for k in 0..attempts {
        let face = faces.partition_point(|(end, _)| *end <= area * random(seed, k, 0) as f64);
        let tri = faces[face].1;
        let [a, b, c] = tri.map(|i| positions[i as usize]);
        let u = random(seed, k, 1).sqrt();
        let v = random(seed, k, 2);
        let weights = [1.0 - u, u * (1.0 - v), u * v, 0.0];
        let pos = a * weights[0] + b * weights[1] + c * weights[2];
        let grid_pos = (pos - origin) / spacing;
        if !grid_pos.is_finite() || grid_pos.abs().max_element() >= 1.0e9 {
            continue;
        }
        let cell = grid_pos.floor().as_ivec3();
        let nearby = (-1..=1)
            .flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| cell + IVec3::new(x, y, z))))
            .filter_map(|key| grid.get(&key))
            .flatten()
            .any(|&other| pos.distance_squared(other) < min_distance);
        if nearby {
            continue;
        }
        grid.entry(cell).or_default().push(pos);
        let edge = if orient == Orient::LongestEdge {
            [(0, 1), (1, 2), (2, 0)]
                .into_iter()
                .map(|(i, j)| [tri[i], tri[j]])
                .max_by(|[a, b], [c, d]| {
                    let distance = |a: u32, b: u32| positions[a as usize].distance_squared(positions[b as usize]);
                    distance(*a, *b).total_cmp(&distance(*c, *d))
                })
                .expect("a triangle has three edges")
        } else {
            [tri[0], tri[1]]
        };
        anchors.push(Anchor { points: [tri[0], tri[1], tri[2], tri[0]], weights, edge, key: k });
        if anchors.len() == MAX_COPIES {
            break;
        }
    }
    anchors
}

/// A number in 0..1 for (`seed`, `key`, `salt`): the same every time.
fn random(seed: u32, key: u32, salt: u32) -> f32 {
    let mut x = (seed as u64) << 32 ^ (key as u64) ^ ((salt as u64) << 56);
    // SplitMix64.
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The source's frame: the point that sits on the surface (the middle of
/// its bottom) and its largest dimension.
pub fn source_frame(source: &Mesh) -> (Vec3, f32) {
    let b = crate::Aabb::from_points(source.positions.iter().copied());
    if b.is_empty() {
        return (Vec3::ZERO, 1.0);
    }
    let base = Vec3::new((b.min.x + b.max.x) * 0.5, b.min.y, (b.min.z + b.max.z) * 0.5);
    (base, b.extent().max_element().max(1e-6))
}

/// Each copy's transform (source space to world), for the host as it is
/// now. Copies whose anchor no longer fits the host are skipped.
pub fn transforms(host: &Mesh, source: &Mesh, anchors: &[Anchor], settings: &Settings) -> Vec<Mat4> {
    let (base, extent) = source_frame(source);
    let vc = host.vertex_count() as u32;
    let normals = &host.normals;
    use rayon::prelude::*;
    anchors
        .par_iter()
        .filter(|a| a.points.iter().chain(&a.edge).all(|&v| v < vc))
        .filter(|a| match settings.place {
            Place::Mask(threshold) => {
                a.points.iter().zip(a.weights).map(|(&v, w)| host.mask[v as usize] * w).sum::<f32>() >= threshold
            }
            _ => true,
        })
        .map(|a| {
            let mut pos = Vec3::ZERO;
            let mut n = Vec3::ZERO;
            for k in 0..4 {
                let w = a.weights[k];
                if w > 0.0 {
                    pos += host.positions[a.points[k] as usize] * w;
                    n += normals.get(a.points[k] as usize).copied().unwrap_or(Vec3::Y) * w;
                }
            }
            let up = n.normalize_or(Vec3::Y);
            // Forward, before laying it onto the surface.
            let along = match settings.orient {
                Orient::World | Orient::Flow | Orient::Random => settings.direction,
                Orient::LongestEdge | Orient::FirstEdge => {
                    host.positions[a.edge[1] as usize] - host.positions[a.edge[0] as usize]
                }
            };
            let mut fwd = along - up * along.dot(up);
            if fwd.length_squared() < 1e-12 {
                fwd = up.any_orthonormal_vector();
            }
            let fwd = fwd.normalize();
            let mut turn = settings.spin.to_radians();
            if settings.orient == Orient::Random {
                turn += random(settings.seed, a.key, 1) * std::f32::consts::TAU;
            }
            turn +=
                (random(settings.seed, a.key, 2) - 0.5) * std::f32::consts::TAU * settings.vary_spin.clamp(0.0, 1.0);
            let fwd = glam::Quat::from_axis_angle(up, turn) * fwd;
            let right = up.cross(fwd).normalize_or(Vec3::X);
            let rot = Mat3::from_cols(right, up, fwd);
            // Size: largest dimension in scene units.
            let reach = match settings.size_mode {
                SizeMode::Absolute => 1.0,
                SizeMode::Face => {
                    let pts: Vec<u32> =
                        a.points.iter().zip(&a.weights).filter(|(_, w)| **w > 0.0).map(|(p, _)| *p).collect();
                    let mut sum = 0.0;
                    let mut count = 0;
                    if pts.len() > 1 {
                        for k in 0..pts.len() {
                            sum += host.positions[pts[k] as usize]
                                .distance(host.positions[pts[(k + 1) % pts.len()] as usize]);
                            count += 1;
                        }
                    } else {
                        let p = host.positions[pts[0] as usize];
                        for q in host.neighbors(pts[0]).iter() {
                            sum += p.distance(host.positions[*q as usize]);
                            count += 1;
                        }
                    }
                    if count > 0 { sum / count as f32 } else { 1.0 }
                }
            };
            let vary = 1.0 + (random(settings.seed, a.key, 3) - 0.5) * settings.vary_size.clamp(0.0, 1.0);
            let size = (settings.size * reach * vary).max(0.0);
            let scale = size / extent;
            let lift = up * (settings.offset * size);
            Mat4::from_translation(pos + lift)
                * Mat4::from_mat3(rot)
                * Mat4::from_scale(Vec3::splat(scale))
                * Mat4::from_translation(-base)
        })
        .collect()
}

/// The copies as real geometry: one mesh with every copy, one face group
/// per copy (so they can be hidden or picked one by one), the source's
/// colors kept.
pub fn realize(source: &Mesh, transforms: &[Mat4]) -> crate::MeshData {
    let src = source.to_data();
    let nv = src.positions.len();
    let nt = src.triangles.len();
    let mut out = crate::MeshData {
        positions: Vec::with_capacity(nv * transforms.len()),
        triangles: Vec::with_capacity(nt * transforms.len()),
        ..Default::default()
    };
    let painted = !src.color.is_empty();
    for (i, m) in transforms.iter().enumerate() {
        let first = out.positions.len() as u32;
        out.positions.extend(src.positions.iter().map(|&p| m.transform_point3(p)));
        // A mirroring transform (negative scale) would turn faces inside out.
        let flip = m.determinant() < 0.0;
        out.triangles.extend(src.triangles.iter().map(|t| {
            let t = t.map(|v| v + first);
            if flip { [t[0], t[2], t[1]] } else { t }
        }));
        out.groups.extend(std::iter::repeat_n(i as u32, nt));
        if painted {
            out.color.extend(src.color.iter().copied());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn stud() -> Mesh {
        // A small cone pointing up +Y: easy to see where it points.
        primitives::cone(8, 0.5, 1.0)
    }

    #[test]
    fn scatter_is_seeded_spaced_and_excludes_hidden_triangles() {
        let positions = [Vec3::ZERO, Vec3::X, Vec3::Z, Vec3::X + Vec3::Z];
        let tris = [[0, 1, 2], [1, 3, 2]];
        let make = |seed| {
            scatter_anchors(
                &positions,
                &tris,
                &[false, true],
                ScatterOpts { spacing: 0.16, seed, orient: Orient::World, ..Default::default() },
            )
        };
        let samples = make(7);
        assert!(samples.len() >= 8);
        assert_eq!(samples, make(7));
        assert_ne!(samples, make(8));
        let points: Vec<_> = samples
            .iter()
            .map(|a| {
                assert_eq!(a.points[..3], tris[0]);
                a.points.iter().zip(a.weights).map(|(&p, w)| positions[p as usize] * w).sum::<Vec3>()
            })
            .collect();
        for (i, p) in points.iter().enumerate() {
            for q in &points[..i] {
                assert!(p.distance(*q) >= 0.16 - 1e-6);
            }
        }
        assert!(
            scatter_anchors(
                &positions,
                &tris,
                &[true, true],
                ScatterOpts { spacing: 0.16, seed: 7, orient: Orient::World, ..Default::default() },
            )
            .is_empty()
        );
    }

    #[test]
    fn scatter_mask_and_group_filters_drop_other_faces() {
        // Two disjoint quads so mask/group filters don't share vertices.
        let positions = [
            Vec3::ZERO,
            Vec3::X,
            Vec3::Z,
            Vec3::X + Vec3::Z,
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(11.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
            Vec3::new(11.0, 0.0, 1.0),
        ];
        let tris = [[0, 1, 2], [1, 3, 2], [4, 5, 6], [5, 7, 6]];
        let hidden = [false; 4];
        let mask = [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let on_mask = scatter_anchors(
            &positions,
            &tris,
            &hidden,
            ScatterOpts {
                spacing: 0.2,
                seed: 3,
                orient: Orient::World,
                mask: Some((&mask, 0.5)),
                ..Default::default()
            },
        );
        assert!(!on_mask.is_empty());
        assert!(on_mask.iter().all(|a| a.points[..3] == tris[0] || a.points[..3] == tris[1]));
        let groups = [0u32, 0, 1, 1];
        let on_g1 = scatter_anchors(
            &positions,
            &tris,
            &hidden,
            ScatterOpts {
                spacing: 0.2,
                seed: 3,
                orient: Orient::World,
                group: Some((&groups, 1)),
                ..Default::default()
            },
        );
        assert!(!on_g1.is_empty());
        assert!(on_g1.iter().all(|a| a.points[..3] == tris[2] || a.points[..3] == tris[3]));
    }

    #[test]
    fn host_flow_picks_longest_aabb_axis() {
        let host = primitives::plane(4, 2.0); // XY plane, extent mostly in X/Z equal; Y thin
        let d = host_flow_direction(&host);
        assert!(d == Vec3::X || d == Vec3::Z, "{d}");
    }

    #[test]
    fn one_copy_per_quad_on_a_quad_mesh_and_one_per_point() {
        let host = primitives::quad_sphere(4, 1.0);
        let quads = host.quad_count();
        let faces = anchors(&host, &Settings::default());
        assert_eq!(faces.len(), quads, "a copy per quad, not per triangle");
        let points = anchors(&host, &Settings { place: Place::Points, ..Default::default() });
        assert_eq!(points.len(), host.vertex_count());
        // Any mesh: one per quad the mesh finds, and one per triangle left.
        let ico = primitives::icosphere(1, 1.0);
        let (q, t) = ico.quad_and_triangle_counts();
        assert_eq!(anchors(&ico, &Settings::default()).len(), q + t);
    }

    #[test]
    fn copies_stand_on_the_surface_pointing_out() {
        let host = primitives::quad_sphere(4, 1.0);
        let src = stud();
        let s = Settings { size_mode: SizeMode::Absolute, size: 0.2, ..Default::default() };
        let a = anchors(&host, &s);
        let ts = transforms(&host, &src, &a, &s);
        assert_eq!(ts.len(), a.len());
        let (base, extent) = source_frame(&src);
        let tip_local = Vec3::new(base.x, base.y + extent, base.z);
        for t in &ts {
            let at = t.transform_point3(base);
            let tip = t.transform_point3(tip_local);
            // The base on (just inside, for a flat quad) the unit sphere,
            // the tip further out along the normal.
            assert!((0.9..=1.01).contains(&at.length()), "base at {at}");
            assert!(tip.length() > at.length() + 0.1, "points outward: {at} -> {tip}");
            // Absolute size: largest dimension 0.2.
            assert!((t.transform_vector3(Vec3::Y * extent).length() - 0.2).abs() < 1e-4);
        }
    }

    #[test]
    fn follows_the_host_when_it_is_sculpted() {
        let mut host = primitives::quad_sphere(3, 1.0);
        let src = stud();
        let s = Settings::default();
        let a = anchors(&host, &s);
        let before = transforms(&host, &src, &a, &s);
        // Push the whole host out (a "sculpt"): same anchors, copies move.
        let moved: Vec<u32> = (0..host.vertex_count() as u32).collect();
        for p in &mut host.positions {
            *p *= 1.5;
        }
        host.update_after_move(&moved);
        let after = transforms(&host, &src, &a, &s);
        let (base, _) = source_frame(&src);
        for (b, c) in before.iter().zip(&after) {
            let (pb, pc) = (b.transform_point3(base), c.transform_point3(base));
            assert!((pc.length() / pb.length() - 1.5).abs() < 0.02, "{pb} -> {pc}");
        }
    }

    #[test]
    fn world_direction_never_flips_and_variation_is_repeatable() {
        let host = primitives::plane(6, 2.0);
        let src = stud();
        let s = Settings::default();
        let a = anchors(&host, &s);
        let ts = transforms(&host, &src, &a, &s);
        // On a flat plane facing +Y, forward is world +Z for every copy.
        for t in &ts {
            let fwd = t.transform_vector3(Vec3::Z).normalize();
            assert!(fwd.dot(Vec3::Z) > 0.999, "{fwd}");
        }
        // Random variation: the same seed gives the same copies, another
        // seed different ones.
        let v = Settings { vary_size: 1.0, vary_spin: 1.0, ..s };
        assert_eq!(transforms(&host, &src, &a, &v), transforms(&host, &src, &a, &v));
        assert_ne!(transforms(&host, &src, &a, &v), transforms(&host, &src, &a, &Settings { seed: 2, ..v }));
    }

    #[test]
    fn group_placement_and_realize() {
        let mut d = primitives::quad_sphere(3, 1.0).to_data();
        // Group 1: the triangles above the equator.
        d.groups = d
            .triangles
            .iter()
            .map(|t| if t.iter().map(|&v| d.positions[v as usize].y).sum::<f32>() > 0.0 { 1 } else { 0 })
            .collect();
        let host = Mesh::from_data(d);
        let all = anchors(&host, &Settings::default());
        let top = anchors(&host, &Settings { place: Place::Group(1), ..Default::default() });
        assert!(!top.is_empty() && top.len() < all.len());
        let src = stud();
        let s = Settings { place: Place::Group(1), ..Default::default() };
        let ts = transforms(&host, &src, &top, &s);
        let real = realize(&src, &ts);
        assert_eq!(real.triangles.len(), src.triangle_count() * ts.len());
        assert_eq!(real.groups.iter().copied().max(), Some(ts.len() as u32 - 1), "a face group per copy");
        // Every copy stands on the top half (its base, the transform's
        // translation; a copy near the equator leans out sideways).
        assert!(ts.iter().all(|t| t.w_axis.y > -0.05), "only on the top half");
        let _ = Mesh::from_data(real);
    }
}
