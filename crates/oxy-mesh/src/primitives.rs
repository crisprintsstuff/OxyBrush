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
}
