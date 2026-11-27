//! Closed, topology-aware cross sections of a triangulated surface.

use crate::{Mesh, Vec3};
use std::collections::{HashMap, HashSet};

/// Cut the visible surface at a plane through `anchor` and resample the
/// closed section nearest the anchor. Open or branched sections cannot
/// define a ring. Start the ring near the anchor so repeated cuts have a
/// useful initial alignment.
pub fn ring(mesh: &Mesh, anchor: Vec3, normal: Vec3, count: usize) -> Option<Vec<Vec3>> {
    let normal = normal.try_normalize()?;
    if count < 3 || !anchor.is_finite() {
        return None;
    }
    let mut points = Vec::new();
    let mut nodes = HashMap::new();
    let mut edges = HashSet::new();
    let mut neighbors: Vec<Vec<usize>> = Vec::new();
    for chunk in &mesh.chunks {
        let half_span = 0.5 * chunk.aabb.extent().dot(normal.abs());
        if (chunk.aabb.center() - anchor).dot(normal).abs() > half_span {
            continue;
        }
        for face in chunk.triangles.clone() {
            if mesh.hidden()[face as usize] {
                continue;
            }
            let tri = mesh.triangles[face as usize];
            let p = tri.map(|v| mesh.positions[v as usize]);
            let d = p.map(|p| (p - anchor).dot(normal));
            let cut: Vec<_> = [(0, 1), (1, 2), (2, 0)]
                .into_iter()
                .filter(|&(a, b)| (d[a] < 0.0) != (d[b] < 0.0))
                .map(|(a, b)| {
                    // When a cut hits a vertex exactly, all incident edges
                    // meet at the same node.
                    let key = if d[a] == 0.0 {
                        [tri[a]; 2]
                    } else if d[b] == 0.0 {
                        [tri[b]; 2]
                    } else {
                        [tri[a].min(tri[b]), tri[a].max(tri[b])]
                    };
                    *nodes.entry(key).or_insert_with(|| {
                        let i = points.len();
                        points.push(p[a].lerp(p[b], d[a] / (d[a] - d[b])));
                        neighbors.push(Vec::new());
                        i
                    })
                })
                .collect();
            if let [a, b] = *cut.as_slice()
                && a != b
                && edges.insert([a.min(b), a.max(b)])
            {
                neighbors[a].push(b);
                neighbors[b].push(a);
            }
        }
    }

    let mut seen = vec![false; points.len()];
    let mut nearest = None;
    for start in 0..points.len() {
        if seen[start] || neighbors[start].len() != 2 {
            continue;
        }
        let mut order = Vec::new();
        let (mut prev, mut at) = (start, neighbors[start][0]);
        loop {
            if seen[at] || neighbors[at].len() != 2 {
                break;
            }
            seen[at] = true;
            order.push(points[at]);
            if at == start {
                let (edge, distance, parameter) = nearest_segment(&order, anchor);
                if nearest.as_ref().is_none_or(|&(_, old, _, _)| distance < old) {
                    nearest = Some((order, distance, edge, parameter));
                }
                break;
            }
            let next = if neighbors[at][0] == prev { neighbors[at][1] } else { neighbors[at][0] };
            (prev, at) = (at, next);
        }
    }
    let (mut contour, distance, edge, parameter) = nearest?;
    // The pointer hit the source on this plane. Don't jump from an open
    // section beneath it to an unrelated closed section elsewhere.
    let tolerance = (mesh.bounds().extent().max_element() * 1e-4).max(1e-6);
    if distance > tolerance * tolerance {
        return None;
    }
    contour.rotate_left(edge);
    let first = contour[0].lerp(contour[1], parameter);
    contour.rotate_left(1);
    resample(&contour, first, count)
}

fn nearest_segment(points: &[Vec3], at: Vec3) -> (usize, f32, f32) {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .enumerate()
        .map(|(index, (&a, &b))| {
            let delta = b - a;
            let t =
                if delta == Vec3::ZERO { 0.0 } else { ((at - a).dot(delta) / delta.length_squared()).clamp(0.0, 1.0) };
            (index, a.lerp(b, t).distance_squared(at), t)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("closed contour")
}

fn resample(points: &[Vec3], first: Vec3, n: usize) -> Option<Vec<Vec3>> {
    let mut path = Vec::with_capacity(points.len() + 2);
    path.push(first);
    path.extend_from_slice(points);
    path.push(first);
    path.dedup();
    let mut distances = Vec::with_capacity(path.len());
    distances.push(0.0);
    for segment in path.windows(2) {
        distances.push(distances.last()? + segment[0].distance(segment[1]));
    }
    let length = *distances.last()?;
    if length <= f32::EPSILON || !length.is_finite() {
        return None;
    }
    let mut index = 0;
    Some(
        (0..n)
            .map(|i| {
                let distance = length * (i as f32 / n as f32);
                while index + 2 < distances.len() && distances[index + 1] <= distance {
                    index += 1;
                }
                let t = (distance - distances[index]) / (distances[index + 1] - distances[index]);
                path[index].lerp(path[index + 1], t)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn selects_nearest_closed_shell_and_samples_at_the_cut() {
        let mesh = primitives::icosphere(3, 1.0);
        let mut positions = mesh.positions.clone();
        let n = positions.len() as u32;
        positions.extend(mesh.positions.iter().map(|p| *p + 3.0 * Vec3::X));
        let triangles = mesh.triangles.iter().copied().chain(mesh.triangles.iter().map(|t| t.map(|v| v + n))).collect();
        let mesh = Mesh::new(positions, triangles);
        let anchor = mesh.raycast(Vec3::new(3.0, 0.13, 3.0), Vec3::NEG_Z).unwrap().point;
        let points = ring(&mesh, anchor, Vec3::Y, 12).unwrap();
        assert_eq!(points.len(), 12);
        assert!(points[0].distance(anchor) < 0.02);
        assert!(points.iter().all(|p| (p.y - anchor.y).abs() < 1e-5 && p.x > 1.9));
        let lengths: Vec<_> = points.iter().zip(points.iter().cycle().skip(1)).map(|(&a, &b)| a.distance(b)).collect();
        assert!(lengths.iter().all(|&x| (x - lengths[0]).abs() < 0.015), "{lengths:?}");
    }

    #[test]
    fn open_or_hidden_sections_do_not_form_rings() {
        let plane = primitives::plane(6, 2.0);
        assert!(ring(&plane, Vec3::ZERO, Vec3::X, 8).is_none());
        let mut mesh = primitives::icosphere(2, 1.0);
        mesh.set_hidden(mesh.triangles.iter().map(|tri| mesh.positions[tri[0] as usize].x < -0.25).collect());
        assert!(ring(&mesh, Vec3::Z, Vec3::Y, 8).is_none());
    }
}
