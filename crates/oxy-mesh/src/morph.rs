//! Position blending for two variants of the same indexed mesh.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::{Mesh, RayHit, Vec3};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixBrush {
    KeepA,
    KeepB,
    Erase,
}

/// How much of the mix is reserved for each endpoint. The remaining weight
/// follows the global slider.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MixWeight {
    a: f32,
    b: f32,
}

impl MixWeight {
    pub fn from_pins([a, b]: [f32; 2]) -> Option<Self> {
        ((0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) && a + b <= 1.0 + f32::EPSILON).then_some(Self { a, b })
    }

    pub fn paint(self, brush: MixBrush, amount: f32) -> Self {
        if !amount.is_finite() {
            return self;
        }
        let amount = amount.clamp(0.0, 1.0);
        let remaining = 1.0 - amount;
        Self {
            a: self.a * remaining + if brush == MixBrush::KeepA { amount } else { 0.0 },
            b: self.b * remaining + if brush == MixBrush::KeepB { amount } else { 0.0 },
        }
    }

    pub fn pins(self) -> [f32; 2] {
        [self.a, self.b]
    }

    pub fn position(self, a: Vec3, b: Vec3, mix: f32) -> Vec3 {
        let factor = self.b + (1.0 - self.a - self.b).max(0.0) * mix.clamp(0.0, 1.0);
        if factor <= 0.0 {
            a
        } else if factor >= 1.0 {
            b
        } else {
            a.lerp(b, factor)
        }
    }
}

/// Blend positions in the shared vertex order. A fork made by cloning a
/// mesh retains its topology version through position edits. A rebuilt or
/// independently loaded mesh needs its own correspondence before blending.
pub fn blend_positions(a: &Mesh, b: &Mesh, mix: f32, weights: &[MixWeight]) -> Option<Vec<Vec3>> {
    if !mix.is_finite() || a.topology_version() != b.topology_version() || a.vertex_count() != b.vertex_count() {
        return None;
    }
    if weights.len() != a.vertex_count() {
        return None;
    }
    Some(
        a.positions
            .par_iter()
            .zip(&b.positions)
            .zip(weights)
            .map(|((&a, &b), &weight)| weight.position(a, b, mix))
            .collect(),
    )
}

/// Match vertices after loading, when each mesh may have been reordered
/// independently. Check the bijection and oriented faces before using it.
pub fn matches_topology(a: &Mesh, b: &Mesh, vertices: &[u32]) -> bool {
    if a.triangle_count() != b.triangle_count()
        || a.vertex_count() != b.vertex_count()
        || a.vertex_count() != vertices.len()
    {
        return false;
    }
    let mut seen = vec![false; vertices.len()];
    for &index in vertices {
        if seen.get_mut(index as usize).is_none_or(|used| std::mem::replace(used, true)) {
            return false;
        }
    }
    let rotate = |[a, b, c]: [u32; 3]| {
        if a < b && a < c {
            [a, b, c]
        } else if b < c {
            [b, c, a]
        } else {
            [c, a, b]
        }
    };
    let mut expected: Vec<_> = a.triangles.iter().map(|t| rotate(t.map(|v| vertices[v as usize]))).collect();
    let mut actual: Vec<_> = b.triangles.iter().copied().map(rotate).collect();
    expected.par_sort_unstable();
    actual.par_sort_unstable();
    expected == actual
}

/// Evaluate with a correspondence that was checked by `matches_topology`.
pub fn blend_matched_positions(a: &Mesh, b: &Mesh, mix: f32, weights: &[MixWeight], vertices: &[u32]) -> Vec<Vec3> {
    a.positions
        .par_iter()
        .zip(weights)
        .zip(vertices)
        .map(|((&a, &weight), &v)| weight.position(a, b.positions[v as usize], mix))
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
struct Hop {
    distance: f32,
    vertex: u32,
}

impl Eq for Hop {}

impl PartialOrd for Hop {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hop {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap pops the nearest vertex first.
        other.distance.total_cmp(&self.distance).then_with(|| self.vertex.cmp(&other.vertex))
    }
}

/// Walk visible triangle edges from the picked face. A point on a different
/// fold may be close in space but is outside the dab if the surface path to
/// it is longer than the radius. Each reached vertex is visited once.
pub fn surface_dab(mesh: &Mesh, hit: RayHit, radius: f32, mut visit: impl FnMut(u32, f32)) {
    if !radius.is_finite() || radius <= 0.0 || mesh.hidden()[hit.triangle as usize] {
        return;
    }
    let mut distances = HashMap::new();
    let mut frontier = BinaryHeap::new();
    let push = |distances: &mut HashMap<u32, f32>, frontier: &mut BinaryHeap<Hop>, vertex, distance| {
        if distance < radius && distances.get(&vertex).is_none_or(|&previous| previous > distance) {
            distances.insert(vertex, distance);
            frontier.push(Hop { distance, vertex });
        }
    };
    for vertex in mesh.triangles[hit.triangle as usize] {
        push(&mut distances, &mut frontier, vertex, hit.point.distance(mesh.positions[vertex as usize]));
    }
    while let Some(Hop { vertex, distance }) = frontier.pop() {
        if distance > distances[&vertex] {
            continue;
        }
        // A flat inner quarter makes a fully pinned region easy to paint.
        let t = ((1.0 - distance / radius) / 0.75).min(1.0);
        visit(vertex, t * t * (3.0 - 2.0 * t));
        let point = mesh.positions[vertex as usize];
        for &triangle in mesh.vertex_triangles(vertex) {
            if mesh.hidden()[triangle as usize] {
                continue;
            }
            for neighbor in mesh.triangles[triangle as usize] {
                let next = distance + point.distance(mesh.positions[neighbor as usize]);
                push(&mut distances, &mut frontier, neighbor, next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn blend_uses_the_same_vertices_and_preserves_exact_endpoints() {
        let a = primitives::cube(2, 1.0);
        let mut b = a.clone();
        b.replace_positions(a.positions.iter().map(|p| *p + Vec3::splat(2.0)).collect());
        let weights = vec![MixWeight::default(); a.vertex_count()];
        assert_eq!(blend_positions(&a, &b, 0.0, &weights).unwrap(), a.positions);
        assert_eq!(blend_positions(&a, &b, 1.0, &weights).unwrap(), b.positions);
        let expected: Vec<_> = a.positions.iter().map(|p| *p + Vec3::splat(0.5)).collect();
        assert_eq!(blend_positions(&a, &b, 0.25, &weights).unwrap(), expected);
        assert!(blend_positions(&a, &b, f32::NAN, &weights).is_none());
        assert!(blend_positions(&a, &primitives::cube(2, 1.0), 0.5, &weights).is_none());
        assert!(blend_positions(&a, &b, 0.25, &[]).is_none());
    }

    #[test]
    fn pins_override_mix_and_erase_restores_it() {
        let blank = MixWeight::default();
        let keep_a = blank.paint(MixBrush::KeepA, 1.0);
        let keep_b = blank.paint(MixBrush::KeepB, 1.0);
        assert_eq!(keep_a.position(Vec3::ZERO, Vec3::X, 1.0), Vec3::ZERO);
        assert_eq!(keep_b.position(Vec3::ZERO, Vec3::X, 0.0), Vec3::X);
        let both = keep_a.paint(MixBrush::KeepB, 0.25);
        assert_eq!(both.pins(), [0.75, 0.25]);
        assert_eq!(both.position(Vec3::ZERO, Vec3::X, 1.0), Vec3::X * 0.25);
        let erased = both.paint(MixBrush::Erase, 1.0);
        assert_eq!(erased.position(Vec3::ZERO, Vec3::X, 0.75), Vec3::X * 0.75);
    }

    #[test]
    fn dab_stays_on_the_picked_side_of_a_fold() {
        // Two nearby sheets join along the far edge. The lower sheet is
        // within the radius in space, but farther away along the surface.
        let positions = vec![
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(0.0, 0.1, 0.0),
            Vec3::new(0.0, 0.0, -0.01),
            Vec3::new(1.0, 0.0, -0.01),
            Vec3::new(0.0, 0.1, -0.01),
        ];
        let mesh = Mesh::from_data(crate::MeshData {
            positions,
            triangles: vec![[0, 1, 2], [5, 4, 3], [1, 4, 2], [2, 4, 5]],
            ..Default::default()
        });
        // Pick near vertex 0, far from either end of the joining edge.
        let hit = mesh.raycast(Vec3::new(0.001, 0.005, 1.0), -Vec3::Z).unwrap();
        let mut vertices = Vec::new();
        surface_dab(&mesh, hit, 0.04, |v, falloff| vertices.push((mesh.positions[v as usize], falloff)));
        assert_eq!(vertices, [(Vec3::ZERO, 1.0)]);
    }
}
