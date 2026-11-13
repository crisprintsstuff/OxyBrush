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

/// A cube's faces, half-size `h`: (origin, u axis, v axis) per face,
/// counter-clockwise when viewed from outside.
fn cube_faces(h: f32) -> [(Vec3, Vec3, Vec3); 6] {
    [
        (Vec3::new(-h, -h, h), Vec3::X, Vec3::Y),
        (Vec3::new(h, -h, -h), -Vec3::X, Vec3::Y),
        (Vec3::new(h, -h, h), -Vec3::Z, Vec3::Y),
        (Vec3::new(-h, -h, -h), Vec3::Z, Vec3::Y),
        (Vec3::new(-h, h, h), Vec3::X, -Vec3::Z),
        (Vec3::new(-h, -h, -h), Vec3::X, Vec3::Z),
    ]
}

/// Quad sphere: a cube with `divisions`² quads per face (each split into two
/// triangles), pushed out onto a sphere of `radius`.
///
/// Every point gets the same angular spacing across its cube face (the
/// "equal-angle" mapping: each coordinate `x` in -1..=1 becomes
/// `tan(x · π/4)` before normalising), so the quads near the cube corners
/// come out close in size to those in the middle of a face. Unlike the
/// icosphere, every point has four neighbours except the eight corners,
/// which have three, and there are no poles as on a UV sphere.
pub fn quad_sphere(divisions: u32, radius: f32) -> Mesh {
    let quarter = std::f32::consts::FRAC_PI_4;
    let onto_sphere =
        |p: Vec3| Vec3::new((p.x * quarter).tan(), (p.y * quarter).tan(), (p.z * quarter).tan()).normalize();
    let mut positions = Vec::new();
    let mut tris = Vec::new();
    for (o, u, v) in cube_faces(1.0) {
        let (p, t) = grid(divisions, |a, b| onto_sphere(o + u * (a * 2.0) + v * (b * 2.0)));
        let base = positions.len() as u32;
        // `grid` gives each quad as the pair [a, b, c], [a, c, d]. Split it
        // along the diagonal between its two narrower corners instead: next
        // to the cube's corners a quad has an angle of up to 120°, and a
        // diagonal ending there makes a pair that quad detection
        // (`topo::find_quad_diagonals`) reads as two triangles.
        for pair in t.as_chunks::<2>().0 {
            let ([a, b, c], d) = (pair[0], pair[1][2]);
            let at = |i: u32| p[i as usize];
            // Cosine of the quad's angle at `v`, between its neighbours.
            let cos = |prev: u32, v: u32, next: u32| (at(prev) - at(v)).normalize().dot((at(next) - at(v)).normalize());
            let widest_on_bd = cos(a, b, c).min(cos(c, d, a));
            let widest_on_ac = cos(d, a, b).min(cos(b, c, d));
            let split = if widest_on_ac < widest_on_bd { [[a, b, d], [b, c, d]] } else { [[a, b, c], [a, c, d]] };
            tris.extend(split.map(|t| t.map(|i| i + base)));
        }
        positions.extend(p);
    }
    let (positions, tris) = weld(positions, tris, 1e-5);
    Mesh::new(positions.into_iter().map(|p| p * radius).collect(), tris)
}

/// UV sphere: `segments` around the Y axis and `rings` from pole to pole,
/// quads everywhere except the triangle fans at the two poles.
pub fn uv_sphere(segments: u32, rings: u32, radius: f32) -> Mesh {
    let rings = rings.max(2);
    let profile: Vec<(f32, f32)> = (0..=rings)
        .map(|i| {
            if i == 0 {
                return (0.0, -radius);
            }
            if i == rings {
                return (0.0, radius);
            }
            let a = std::f32::consts::PI * i as f32 / rings as f32 - std::f32::consts::FRAC_PI_2;
            (radius * a.cos(), radius * a.sin())
        })
        .collect();
    lathe(&profile, segments, false, false)
}

/// Square grid in the XZ plane, facing +Y.
pub fn plane(divisions: u32, size: f32) -> Mesh {
    let (p, t) = grid(divisions, |u, v| Vec3::new((u - 0.5) * size, 0.0, (0.5 - v) * size));
    Mesh::new(p, t)
}

/// Cube with each face divided into `divisions`² quads (as triangles).
pub fn cube(divisions: u32, size: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut tris = Vec::new();
    for (o, u, v) in cube_faces(size * 0.5) {
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

/// A fast hash for `weld`'s pre-mixed u64 cell keys (it looks up to 8 cells
/// per point, and the default SipHash made a million-triangle STL import
/// several times slower).
#[derive(Clone, Copy, Default)]
struct CellHash;

impl std::hash::BuildHasher for CellHash {
    type Hasher = CellHasher;
    fn build_hasher(&self) -> CellHasher {
        CellHasher(0)
    }
}

struct CellHasher(u64);

impl std::hash::Hasher for CellHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, v: u64) {
        // Keys are already mixed; spread the high bits into the low ones.
        self.0 = v ^ (v >> 29);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

/// Merge vertices closer than `epsilon` (grid-hash based).
///
/// Points are hashed into cells `2 * epsilon` wide. A point within
/// `epsilon` of `p` lies in `p`'s cell or, on each axis, in the neighbour
/// on the side `p` is nearer to, so at most 8 cells are searched, `p`'s own
/// first. Hashing alone isn't enough: two copies of one point that differ by
/// a rounding error can fall on either side of a cell boundary, and were
/// then never merged (the 64-division cube that File > New > Cube makes had
/// 128 open seam edges that way).
pub fn weld(positions: Vec<Vec3>, tris: Vec<[u32; 3]>, epsilon: f32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let (out, tris, _) = weld_with_map(positions, tris, epsilon);
    (out, tris)
}

/// [`weld`], also returning where each input point went: input point `i`
/// is output point `map[i]` (for carrying per-point data such as color).
pub fn weld_with_map(positions: Vec<Vec3>, tris: Vec<[u32; 3]>, epsilon: f32) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<u32>) {
    let epsilon = epsilon.max(f32::MIN_POSITIVE);
    let inv = 0.5 / epsilon;
    // Each cell's kept points as a linked list: `head` per cell, `next` per point.
    // Cells are folded into one u64; two cells sharing a key only means
    // more candidates, each still checked by distance.
    let key = |c: [i64; 3]| {
        (c[0] as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (c[1] as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
            ^ (c[2] as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
    };
    let mut head: HashMap<u64, u32, CellHash> = HashMap::with_capacity_and_hasher(positions.len(), CellHash);
    let mut next: Vec<u32> = Vec::with_capacity(positions.len());
    let mut out: Vec<Vec3> = Vec::with_capacity(positions.len());
    let remap: Vec<u32> = positions
        .iter()
        .map(|&p| {
            let f = p * inv;
            let c = [f.x.floor() as i64, f.y.floor() as i64, f.z.floor() as i64];
            let side = |v: f32, c: i64| if v - c as f32 >= 0.5 { 1 } else { -1 };
            let s = [side(f.x, c[0]), side(f.y, c[1]), side(f.z, c[2])];
            for k in 0..8 {
                let cell = [c[0] + s[0] * (k & 1), c[1] + s[1] * (k >> 1 & 1), c[2] + s[2] * (k >> 2 & 1)];
                let mut at = head.get(&key(cell)).copied().unwrap_or(u32::MAX);
                while at != u32::MAX {
                    if out[at as usize].distance(p) <= epsilon {
                        return at;
                    }
                    at = next[at as usize];
                }
            }
            let i = out.len() as u32;
            out.push(p);
            next.push(head.insert(key(c), i).unwrap_or(u32::MAX));
            i
        })
        .collect();
    let tris = tris.into_iter().map(|t| t.map(|i| remap[i as usize])).collect();
    (out, tris, remap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cube_resolution_is_closed_and_manifold() {
        // Seam points that differ by a rounding error must still weld, at
        // every resolution, including the 64 that File > New > Cube uses.
        for div in [1, 2, 3, 7, 16, 63, 64, 100, 128] {
            let m = cube(div, 1.6);
            let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
            for t in &m.triangles {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *edges.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
            assert!(edges.values().all(|&n| n == 2), "div {div}: every edge has two faces");
            let n = div as usize;
            assert_eq!(m.vertex_count(), 6 * n * n + 2, "div {div}: seam points shared");
        }
    }

    #[test]
    fn weld_merges_points_on_either_side_of_a_cell_boundary() {
        let eps = 1e-3;
        // 0.0005 is exactly a cell boundary of the old rounding scheme.
        let a = Vec3::new(0.0005 - 1e-6, 0.0, 0.0);
        let b = Vec3::new(0.0005 + 1e-6, 0.0, 0.0);
        let c = Vec3::new(1.0, 0.0, 0.0);
        let (p, t) = weld(vec![a, b, c], vec![[0, 1, 2]], eps);
        assert_eq!(p.len(), 2);
        assert_eq!(t[0][0], t[0][1]);
    }

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
    fn quad_sphere_is_a_closed_even_round_ball() {
        for n in [1, 4, 12] {
            let m = quad_sphere(n, 2.0);
            let n = n as usize;
            assert_eq!(m.vertex_count(), 6 * n * n + 2);
            assert_eq!(m.triangle_count(), 12 * n * n);
            let (v, f) = (m.vertex_count() as i64, m.triangle_count() as i64);
            assert_eq!(v - f * 3 / 2 + f, 2, "closed, genus 0");
            assert!(m.positions.iter().all(|p| (p.length() - 2.0).abs() < 1e-5));
            assert!(super::tube_tests::signed_volume(&m) > 0.0, "faces point out");
        }
        // Equal-angle spacing: triangles near a cube corner are close in
        // size to those in the middle of a face (plain normalising gives
        // more than 5:1).
        let m = quad_sphere(16, 1.0);
        let areas: Vec<f32> = m
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| m.positions[v as usize]);
                (b - a).cross(c - a).length()
            })
            .collect();
        let (lo, hi) = areas.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        assert!(hi / lo < 2.0, "area ratio {}", hi / lo);
        // Every quad reads back as a quad, the ones at the corners too.
        for n in [3, 4, 7, 32] {
            let m = quad_sphere(n, 1.0);
            let n = n as usize;
            assert_eq!(crate::topo::quad_and_triangle_counts(&m.positions, &m.triangles), (6 * n * n, 0), "{n}");
        }
        let volume = super::tube_tests::signed_volume(&quad_sphere(32, 1.0));
        assert!((volume - 4.0 / 3.0 * std::f32::consts::PI).abs() < 0.02, "{volume}");
    }

    #[test]
    fn uv_sphere_has_poles_rings_and_quads() {
        let m = uv_sphere(24, 12, 1.5);
        assert_eq!(m.vertex_count(), 24 * 11 + 2);
        assert_eq!(m.triangle_count(), 2 * 24 * 10 + 2 * 24);
        let (v, f) = (m.vertex_count() as i64, m.triangle_count() as i64);
        assert_eq!(v - f * 3 / 2 + f, 2, "closed, genus 0");
        assert!(m.positions.iter().all(|p| (p.length() - 1.5).abs() < 1e-5));
        assert!(super::tube_tests::signed_volume(&m) > 0.0, "faces point out");
        // A large radius still gets single pole points.
        assert_eq!(uv_sphere(8, 4, 50.0).vertex_count(), 8 * 3 + 2);
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

    pub(super) fn signed_volume(m: &Mesh) -> f32 {
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

/// A surface of revolution about the Y axis: `profile` is a list of
/// (radius, height) points from bottom to top, revolved with `segments`
/// vertices around. A point with radius 0 becomes a single pole vertex.
/// With `cap_bottom` / `cap_top`, an open end (radius > 0) is closed with
/// a flat fan. Faces point outward when the profile runs bottom to top.
///
/// PRECONDITION: at least two profile points.
pub fn lathe(profile: &[(f32, f32)], segments: u32, cap_bottom: bool, cap_top: bool) -> Mesh {
    let n = segments.max(3) as usize;
    let mut positions = Vec::new();
    // Per profile point: Some(first vertex of its ring) or None for a pole (its vertex index in .1).
    let mut rows: Vec<(bool, u32)> = Vec::with_capacity(profile.len());
    for &(r, y) in profile {
        let start = positions.len() as u32;
        if r <= 1e-7 {
            positions.push(Vec3::new(0.0, y, 0.0));
            rows.push((false, start));
        } else {
            for k in 0..n {
                let a = std::f32::consts::TAU * k as f32 / n as f32;
                positions.push(Vec3::new(r * a.cos(), y, -r * a.sin()));
            }
            rows.push((true, start));
        }
    }
    let at = |row: (bool, u32), k: usize| if row.0 { row.1 + (k % n) as u32 } else { row.1 };
    let mut tris = Vec::new();
    for w in rows.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        for k in 0..n {
            let (a, b, c, d) = (at(lo, k), at(lo, k + 1), at(hi, k), at(hi, k + 1));
            match (lo.0, hi.0) {
                (true, true) => {
                    tris.push([a, b, d]);
                    tris.push([a, d, c]);
                }
                (true, false) => tris.push([a, b, c]),
                (false, true) => tris.push([a, d, c]),
                (false, false) => {}
            }
        }
    }
    let mut cap = |row: (bool, u32), y: f32, top: bool| {
        if !row.0 {
            return;
        }
        let centre = positions.len() as u32;
        positions.push(Vec3::new(0.0, y, 0.0));
        for k in 0..n {
            let (a, b) = (at(row, k), at(row, k + 1));
            tris.push(if top { [centre, a, b] } else { [centre, b, a] });
        }
    };
    if cap_bottom {
        cap(rows[0], profile[0].1, false);
    }
    if cap_top {
        cap(*rows.last().unwrap(), profile.last().unwrap().1, true);
    }
    Mesh::new(positions, tris)
}

/// Cylinder along Y, centred at the origin, with `height_segments` rows of
/// quads up its side and optional flat caps.
pub fn cylinder_with(segments: u32, height_segments: u32, radius: f32, height: f32, caps: bool) -> Mesh {
    let rows = height_segments.max(1);
    let profile: Vec<(f32, f32)> =
        (0..=rows).map(|i| (radius, -height * 0.5 + height * i as f32 / rows as f32)).collect();
    lathe(&profile, segments, caps, caps)
}

/// Cone along Y, centred at the origin, apex up, with `height_segments`
/// rows up its side and an optional flat base.
pub fn cone_with(segments: u32, height_segments: u32, radius: f32, height: f32, cap: bool) -> Mesh {
    let rows = height_segments.max(1);
    let profile: Vec<(f32, f32)> = (0..=rows)
        .map(|i| {
            let t = i as f32 / rows as f32;
            (radius * (1.0 - t), -height * 0.5 + height * t)
        })
        .collect();
    lathe(&profile, segments, cap, false)
}

#[cfg(test)]
mod lathe_tests {
    use super::*;

    fn closed_and_volume(m: &Mesh) -> (bool, f32) {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &m.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let v: f32 = m
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| m.positions[v as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        (edges.values().all(|&n| n == 2), v)
    }

    #[test]
    fn low_poly_cylinder_and_cone_are_closed_and_face_outward() {
        let c = cylinder_with(8, 1, 1.0, 2.0, true);
        assert_eq!(c.vertex_count(), 8 * 2 + 2);
        assert_eq!(c.triangle_count(), 8 * 2 + 8 * 2);
        let (closed, v) = closed_and_volume(&c);
        // An octagonal prism: area of the octagon (2√2) times 2.
        assert!(closed && (v - 2.0 * 2.0 * 2f32.sqrt()).abs() < 1e-3, "{closed} {v}");
        let c = cylinder_with(24, 5, 0.5, 1.0, true);
        assert_eq!(c.vertex_count(), 24 * 6 + 2);
        assert!(closed_and_volume(&c).0);
        let k = cone_with(16, 3, 1.0, 1.5, true);
        let (closed, v) = closed_and_volume(&k);
        assert!(closed && v > 0.0 && v < std::f32::consts::PI * 1.5 / 3.0, "{closed} {v}");
        // Without caps it is open, and still facing out.
        let open = cylinder_with(12, 2, 1.0, 1.0, false);
        assert!(!closed_and_volume(&open).0);
        assert!(open.positions.iter().all(|p| p.is_finite()));
    }
}
