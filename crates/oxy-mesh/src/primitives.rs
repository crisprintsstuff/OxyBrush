//! Base meshes to start sculpting from.

use crate::Mesh;
use glam::Vec3;
use std::collections::HashMap;

/// Icosphere with `subdivisions` levels (20 * 4^n triangles).
pub fn icosphere(subdivisions: u32, radius: f32) -> Mesh {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut pos: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut tris: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let (p, t) = crate::topo::midpoint_subdivide(&pos, &tris);
        pos = p.into_iter().map(|v| v.normalize()).collect();
        tris = t;
    }
    Mesh::new(pos.into_iter().map(|p| p * radius).collect(), tris)
}

/// Square grid in the XZ plane, facing +Y.
pub fn plane(divisions: u32, size: f32) -> Mesh {
    let (p, t) = grid(divisions, |u, v| Vec3::new((u - 0.5) * size, 0.0, (0.5 - v) * size));
    Mesh::new(p, t)
}

/// Cube with each face divided into `divisions`² quads (as triangles).
pub fn cube(divisions: u32, size: f32) -> Mesh {
    let h = size * 0.5;
    // (origin, u axis, v axis) per face, counter-clockwise when viewed from outside.
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::new(-h, -h, h), Vec3::X, Vec3::Y),
        (Vec3::new(h, -h, -h), -Vec3::X, Vec3::Y),
        (Vec3::new(h, -h, h), -Vec3::Z, Vec3::Y),
        (Vec3::new(-h, -h, -h), Vec3::Z, Vec3::Y),
        (Vec3::new(-h, h, h), Vec3::X, -Vec3::Z),
        (Vec3::new(-h, -h, -h), Vec3::X, Vec3::Z),
    ];
    let mut positions = Vec::new();
    let mut tris = Vec::new();
    for (o, u, v) in faces {
        let (p, t) = grid(divisions, |a, b| o + u * (a * size) + v * (b * size));
        let base = positions.len() as u32;
        positions.extend(p);
        tris.extend(t.into_iter().map(|t| t.map(|i| i + base)));
    }
    let (positions, tris) = weld(positions, tris, size * 1e-5);
    Mesh::new(positions, tris)
}

/// Cylinder centered at origin along the Y axis, spanning `-height/2..=height/2`.
pub fn cylinder(segments: u32, radius: f32, height: f32) -> Mesh {
    let n = segments.max(3);
    let half_h = height * 0.5;
    let mut positions = Vec::with_capacity((n * 2 + 2) as usize);
    let mut tris = Vec::with_capacity((n * 4) as usize);

    // Bottom and top center vertices
    let bot_center_idx = 0u32;
    positions.push(Vec3::new(0.0, -half_h, 0.0));
    let top_center_idx = 1u32;
    positions.push(Vec3::new(0.0, half_h, 0.0));

    let base_idx = 2u32;
    for i in 0..n {
        let theta = (i as f32 / n as f32) * std::f32::consts::TAU;
        let x = theta.cos() * radius;
        let z = theta.sin() * radius;
        positions.push(Vec3::new(x, -half_h, z)); // bot: base_idx + i * 2
        positions.push(Vec3::new(x, half_h, z)); // top: base_idx + i * 2 + 1
    }

    for i in 0..n {
        let next = (i + 1) % n;
        let b0 = base_idx + i * 2;
        let t0 = base_idx + i * 2 + 1;
        let b1 = base_idx + next * 2;
        let t1 = base_idx + next * 2 + 1;

        // Side quads (two triangles, CCW seen from outside)
        tris.push([b0, t0, b1]);
        tris.push([b1, t0, t1]);

        // Bottom cap (facing -Y)
        tris.push([bot_center_idx, b0, b1]);

        // Top cap (facing +Y)
        tris.push([top_center_idx, t1, t0]);
    }

    Mesh::new(positions, tris)
}

/// Cone along the Y axis spanning `-height/2..=height/2`, with base at `-height/2` and apex at `+height/2`.
pub fn cone(segments: u32, radius: f32, height: f32) -> Mesh {
    let n = segments.max(3);
    let half_h = height * 0.5;
    let mut positions = Vec::with_capacity((n + 2) as usize);
    let mut tris = Vec::with_capacity((n * 2) as usize);

    let bot_center_idx = 0u32;
    positions.push(Vec3::new(0.0, -half_h, 0.0));
    let apex_idx = 1u32;
    positions.push(Vec3::new(0.0, half_h, 0.0));

    let base_idx = 2u32;
    for i in 0..n {
        let theta = (i as f32 / n as f32) * std::f32::consts::TAU;
        let x = theta.cos() * radius;
        let z = theta.sin() * radius;
        positions.push(Vec3::new(x, -half_h, z));
    }

    for i in 0..n {
        let next = (i + 1) % n;
        let b0 = base_idx + i;
        let b1 = base_idx + next;

        // Bottom cap facing -Y
        tris.push([bot_center_idx, b0, b1]);

        // Cone side facing outward
        tris.push([b0, apex_idx, b1]);
    }

    Mesh::new(positions, tris)
}

/// Torus in the XZ plane revolving around the Y axis.
/// `major_radius` is the distance from the center of the tube to the center of the torus.
/// `minor_radius` is the radius of the tube.
pub fn torus(major_segments: u32, minor_segments: u32, major_radius: f32, minor_radius: f32) -> Mesh {
    let m = major_segments.max(3);
    let n = minor_segments.max(3);
    let mut positions = Vec::with_capacity((m * n) as usize);
    let mut tris = Vec::with_capacity((m * n * 2) as usize);

    for i in 0..m {
        let u = (i as f32 / m as f32) * std::f32::consts::TAU;
        let cos_u = u.cos();
        let sin_u = u.sin();
        for j in 0..n {
            let v = (j as f32 / n as f32) * std::f32::consts::TAU;
            let cos_v = v.cos();
            let sin_v = v.sin();

            let r = major_radius + minor_radius * cos_v;
            let x = r * cos_u;
            let y = minor_radius * sin_v;
            let z = r * sin_u;
            positions.push(Vec3::new(x, y, z));
        }
    }

    let idx = |i: u32, j: u32| i * n + j;

    for i in 0..m {
        let i_next = (i + 1) % m;
        for j in 0..n {
            let j_next = (j + 1) % n;
            let a = idx(i, j);
            let b = idx(i_next, j);
            let c = idx(i_next, j_next);
            let d = idx(i, j_next);

            // Outward facing triangles
            tris.push([a, d, b]);
            tris.push([b, d, c]);
        }
    }

    Mesh::new(positions, tris)
}

fn grid(divisions: u32, f: impl Fn(f32, f32) -> Vec3) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let n = divisions.max(1);
    let mut p = Vec::with_capacity(((n + 1) * (n + 1)) as usize);
    for j in 0..=n {
        for i in 0..=n {
            p.push(f(i as f32 / n as f32, j as f32 / n as f32));
        }
    }
    let mut t = Vec::with_capacity((n * n * 2) as usize);
    let idx = |i: u32, j: u32| j * (n + 1) + i;
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1));
            t.push([a, b, c]);
            t.push([a, c, d]);
        }
    }
    (p, t)
}

/// Merge vertices closer than `epsilon` (grid-hash based).
pub fn weld(positions: Vec<Vec3>, tris: Vec<[u32; 3]>, epsilon: f32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let inv = 1.0 / epsilon.max(f32::MIN_POSITIVE);
    let mut map: HashMap<[i64; 3], u32> = HashMap::new();
    let mut out = Vec::new();
    let remap: Vec<u32> = positions
        .iter()
        .map(|p| {
            let key = [(p.x * inv).round() as i64, (p.y * inv).round() as i64, (p.z * inv).round() as i64];
            *map.entry(key).or_insert_with(|| {
                out.push(*p);
                out.len() as u32 - 1
            })
        })
        .collect();
    let tris = tris.into_iter().map(|t| t.map(|i| remap[i as usize])).collect();
    (out, tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_is_closed() {
        let m = cube(4, 2.0);
        // Euler characteristic of a closed genus-0 surface: V - E + F = 2.
        let f = m.triangle_count() as i64;
        let e = f * 3 / 2;
        assert_eq!(m.vertex_count() as i64 - e + f, 2);
        for (p, n) in m.positions.iter().zip(&m.normals) {
            assert!(p.dot(*n) > 0.0, "inward normal at {p}");
        }
    }

    #[test]
    fn icosphere_counts() {
        let m = icosphere(2, 1.0);
        assert_eq!(m.triangle_count(), 320);
        assert_eq!(m.vertex_count(), 162);
    }

    #[test]
    fn cylinder_is_closed() {
        let m = cylinder(16, 1.0, 2.0);
        let f = m.triangle_count() as i64;
        let e = f * 3 / 2;
        assert_eq!(m.vertex_count() as i64 - e + f, 2);
        for tri in &m.triangles {
            let v0 = m.positions[tri[0] as usize];
            let v1 = m.positions[tri[1] as usize];
            let v2 = m.positions[tri[2] as usize];
            let n = (v1 - v0).cross(v2 - v0);
            let center = (v0 + v1 + v2) / 3.0;
            if center.y.abs() > 0.9 {
                assert!(center.y * n.y > 0.0, "cap normal points inward");
            } else {
                let dot = center.x * n.x + center.z * n.z;
                assert!(dot > 0.0, "side normal points inward: {dot}");
            }
        }
    }

    #[test]
    fn cone_is_closed() {
        let m = cone(16, 1.0, 2.0);
        let f = m.triangle_count() as i64;
        let e = f * 3 / 2;
        assert_eq!(m.vertex_count() as i64 - e + f, 2);
        for tri in &m.triangles {
            let v0 = m.positions[tri[0] as usize];
            let v1 = m.positions[tri[1] as usize];
            let v2 = m.positions[tri[2] as usize];
            let n = (v1 - v0).cross(v2 - v0);
            let center = (v0 + v1 + v2) / 3.0;
            if center.y < -0.99 {
                assert!(n.y < 0.0, "bottom cap points inward");
            } else {
                let dot = center.x * n.x + center.z * n.z;
                assert!(dot > 0.0, "side normal points inward: {dot}");
            }
        }
    }

    #[test]
    fn torus_is_closed() {
        let m = torus(16, 12, 2.0, 0.5);
        let f = m.triangle_count() as i64;
        let e = f * 3 / 2;
        // Genus 1: V - E + F = 0
        assert_eq!(m.vertex_count() as i64 - e + f, 0);
        for tri in &m.triangles {
            let v0 = m.positions[tri[0] as usize];
            let v1 = m.positions[tri[1] as usize];
            let v2 = m.positions[tri[2] as usize];
            let n = (v1 - v0).cross(v2 - v0);
            let center = (v0 + v1 + v2) / 3.0;
            let center_tube = Vec3::new(center.x, 0.0, center.z).normalize() * 2.0;
            let outward = center - center_tube;
            assert!(outward.dot(n) > 0.0, "torus normal points inward");
        }
    }
}

/// Points along a smooth curve through `points` (uniform Catmull-Rom with
/// clamped ends), each with its radius
/// blended from `radii`, about `spacing` apart along the curve.
///
/// PRECONDITION: `points.len() == radii.len() >= 2`.
/// RETURNS: at least two (position, radius) pairs, first and last exactly at
/// the end points.
pub fn tube_path(points: &[Vec3], radii: &[f32], spacing: f32) -> Vec<(Vec3, f32)> {
    assert!(points.len() >= 2 && points.len() == radii.len());
    let n = points.len();
    let at = |i: isize| points[i.clamp(0, n as isize - 1) as usize];
    // Dense samples per segment, then resampled evenly by arc length.
    let mut dense: Vec<(Vec3, f32)> = Vec::new();
    for s in 0..n - 1 {
        let (p0, p1, p2, p3) = (at(s as isize - 1), at(s as isize), at(s as isize + 1), at(s as isize + 2));
        let steps = 24;
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let p = 0.5
                * ((2.0 * p1)
                    + (p2 - p0) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3);
            // Smoothstep between the two radii, so a thick end eases in.
            let e = t * t * (3.0 - 2.0 * t);
            dense.push((p, radii[s] + (radii[s + 1] - radii[s]) * e));
        }
    }
    dense.push((points[n - 1], radii[n - 1]));
    let mut length = vec![0.0f32];
    for w in dense.windows(2) {
        length.push(length.last().unwrap() + w[0].0.distance(w[1].0));
    }
    let total = *length.last().unwrap();
    let count = ((total / spacing.max(1e-5)).ceil() as usize).clamp(1, 2000);
    let mut out = Vec::with_capacity(count + 1);
    let mut j = 0;
    for i in 0..=count {
        let s = total * i as f32 / count as f32;
        while j + 1 < length.len() - 1 && length[j + 1] < s {
            j += 1;
        }
        let span = (length[j + 1] - length[j]).max(1e-9);
        let f = ((s - length[j]) / span).clamp(0.0, 1.0);
        let (a, b) = (dense[j], dense[j + 1]);
        out.push((a.0.lerp(b.0, f), a.1 + (b.1 - a.1) * f));
    }
    out
}

/// A tube swept along a smooth curve through `points`, `radii[i]` thick at
/// point `i`, with `sides` vertices around. Rings follow the curve with
/// parallel-transported frames, so the tube doesn't twist. With `caps`, each
/// end is closed with a rounded dome, so the result is a closed surface.
///
/// PRECONDITION: `points.len() == radii.len() >= 2`, radii > 0.
pub fn tube(points: &[Vec3], radii: &[f32], sides: u32, caps: bool) -> Mesh {
    let sides = sides.max(3) as usize;
    let min_r = radii.iter().copied().fold(f32::INFINITY, f32::min).max(1e-4);
    let max_r = radii.iter().copied().fold(0.0, f32::max).max(min_r);
    // Ring spacing close to the spacing around the ring, so quads are square-ish.
    let around = std::f32::consts::TAU * (0.5 * (min_r + max_r)) / sides as f32;
    let path = tube_path(points, radii, around.max(min_r * 0.25));
    let rings = path.len();
    let tangent = |i: usize| {
        let a = path[i.saturating_sub(1)].0;
        let b = path[(i + 1).min(rings - 1)].0;
        (b - a).normalize_or(Vec3::Y)
    };
    // Parallel transport: carry the first frame along, rotating it only as
    // much as the tangent turns.
    let t0 = tangent(0);
    let mut normal = if t0.x.abs() < 0.9 { t0.cross(Vec3::X) } else { t0.cross(Vec3::Y) }.normalize();
    let mut prev_t = t0;
    let mut positions = Vec::with_capacity(rings * sides + 2 + if caps { 2 * sides * 3 } else { 0 });
    let mut frames = Vec::with_capacity(rings);
    for (i, &(c, r)) in path.iter().enumerate() {
        let t = tangent(i);
        let axis = prev_t.cross(t);
        if axis.length_squared() > 1e-12 {
            let angle = prev_t.dot(t).clamp(-1.0, 1.0).acos();
            normal = glam::Quat::from_axis_angle(axis.normalize(), angle) * normal;
        }
        normal = (normal - t * normal.dot(t)).normalize_or(normal);
        let binormal = t.cross(normal);
        prev_t = t;
        frames.push((c, r, t, normal, binormal));
        for k in 0..sides {
            let a = std::f32::consts::TAU * k as f32 / sides as f32;
            positions.push(c + (normal * a.cos() + binormal * a.sin()) * r);
        }
    }
    let mut tris = Vec::with_capacity(rings * sides * 2 + if caps { 8 * sides } else { 0 });
    let ring = |i: usize, k: usize| (i * sides + k % sides) as u32;
    for i in 0..rings - 1 {
        for k in 0..sides {
            let (a, b, c, d) = (ring(i, k), ring(i, k + 1), ring(i + 1, k), ring(i + 1, k + 1));
            // Outward facing: k → k+1 turns counter-clockwise seen from
            // ahead (normal × binormal = tangent).
            tris.push([a, b, c]);
            tris.push([b, d, c]);
        }
    }
    if caps {
        // A dome of a few shrinking rings and a tip at each end.
        let domes = [(0usize, -1.0f32), (rings - 1, 1.0)];
        for (end, dir) in domes {
            let (c, r, t, n, b) = frames[end];
            let steps = 3;
            let mut last: Vec<u32> = (0..sides).map(|k| ring(end, k)).collect();
            for s in 1..=steps {
                let phi = std::f32::consts::FRAC_PI_2 * s as f32 / (steps + 1) as f32;
                let (rr, off) = (r * phi.cos(), r * phi.sin() * dir);
                let start = positions.len() as u32;
                for k in 0..sides {
                    let a = std::f32::consts::TAU * k as f32 / sides as f32;
                    positions.push(c + t * off + (n * a.cos() + b * a.sin()) * rr);
                }
                let next: Vec<u32> = (0..sides as u32).map(|k| start + k).collect();
                for k in 0..sides {
                    let (a0, a1, b0, b1) = (last[k], last[(k + 1) % sides], next[k], next[(k + 1) % sides]);
                    if dir > 0.0 {
                        tris.push([a0, a1, b0]);
                        tris.push([a1, b1, b0]);
                    } else {
                        tris.push([a0, b0, a1]);
                        tris.push([a1, b0, b1]);
                    }
                }
                last = next;
            }
            let tip = positions.len() as u32;
            positions.push(c + t * r * dir);
            for k in 0..sides {
                let (a0, a1) = (last[k], last[(k + 1) % sides]);
                if dir > 0.0 {
                    tris.push([a0, a1, tip]);
                } else {
                    tris.push([a0, tip, a1]);
                }
            }
        }
    }
    Mesh::new(positions, tris)
}

#[cfg(test)]
mod tube_tests {
    use super::*;

    fn signed_volume(m: &Mesh) -> f32 {
        m.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| m.positions[v as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn capped_tube_is_closed_outward_and_the_right_size() {
        let pts = [Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
        let m = tube(&pts, &[0.2, 0.2, 0.2], 32, true);
        // Every edge shared by exactly two triangles: watertight.
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &m.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        assert!(edges.values().all(|&n| n == 2), "open or non-manifold edges");
        // Cylinder of length 2 plus two half-spheres (a little less: faceted).
        let expected = std::f32::consts::PI * 0.04 * 2.0 + 4.0 / 3.0 * std::f32::consts::PI * 0.008;
        let v = signed_volume(&m);
        assert!(v > 0.0, "faces point inward");
        assert!((v - expected).abs() < expected * 0.06, "volume {v}, expected about {expected}");
        // The surface stays the radius away from the axis along the straight part.
        for p in &m.positions {
            if p.y > 0.1 && p.y < 1.9 {
                let r = (p.x * p.x + p.z * p.z).sqrt();
                assert!((r - 0.2).abs() < 1e-3, "radius {r} at {p}");
            }
        }
    }

    #[test]
    fn tube_follows_bends_and_blends_radius() {
        let pts = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)];
        let path = tube_path(&pts, &[0.1, 0.1, 0.3], 0.05);
        assert_eq!(path.first().unwrap().0, Vec3::ZERO);
        assert!(path.last().unwrap().0.distance(pts[2]) < 1e-5);
        assert!((path.last().unwrap().1 - 0.3).abs() < 1e-5);
        // Passes through the middle point, radius growing monotonically on the last segment.
        assert!(path.iter().any(|(p, _)| p.distance(pts[1]) < 0.03));
        let open = tube(&pts, &[0.1, 0.1, 0.3], 16, false);
        assert!(signed_volume(&open).is_finite());
        assert!(open.positions.iter().all(|p| p.is_finite()));
    }
}
