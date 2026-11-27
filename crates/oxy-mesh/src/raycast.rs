//! Acceleration for repeated rays against an unchanged mesh.

use crate::{Aabb, Mesh, RayHit, Vec3, line_triangle};
use rayon::prelude::*;

const LEAF_TRIANGLES: usize = 32;

/// A hierarchy over contiguous ranges of the mesh's Morton-ordered
/// triangles. The scene projection brush reuses it across every dab and
/// preview in a gesture.
pub struct RaycastIndex {
    nodes: Vec<Aabb>,
    leaf_start: usize,
}

impl RaycastIndex {
    pub fn new(mesh: &Mesh) -> Self {
        let leaves = mesh.triangle_count().div_ceil(LEAF_TRIANGLES);
        let leaf_start = leaves.next_power_of_two();
        let mut nodes = vec![Aabb::EMPTY; 2 * leaf_start];
        let fill_leaf = |(i, bounds): (usize, &mut Aabb)| {
            let start = i * LEAF_TRIANGLES;
            let end = (start + LEAF_TRIANGLES).min(mesh.triangle_count());
            for t in start..end {
                if !mesh.hidden[t] {
                    for &v in &mesh.triangles[t] {
                        bounds.grow(mesh.positions[v as usize]);
                    }
                }
            }
        };
        let populated = &mut nodes[leaf_start..leaf_start + leaves];
        if leaves >= 1024 {
            populated.par_iter_mut().enumerate().for_each(fill_leaf);
        } else {
            populated.iter_mut().enumerate().for_each(fill_leaf);
        }
        for node in (1..leaf_start).rev() {
            nodes[node] = nodes[2 * node].union(&nodes[2 * node + 1]);
        }
        Self { nodes, leaf_start }
    }

    /// Closest visible hit in either direction. The returned `t` is signed.
    /// Pass the same unchanged mesh used to build the index.
    pub fn nearest_on_line(&self, mesh: &Mesh, origin: Vec3, direction: Vec3) -> Option<RayHit> {
        let ray = Ray { origin, direction, inverse: direction.recip(), parallel: direction.cmpeq(Vec3::ZERO) };
        let entry = ray.entry(self.nodes[1])?;
        let mut hit = None;
        self.visit(mesh, 1, entry, &ray, &mut hit);
        hit
    }

    fn visit(&self, mesh: &Mesh, node: usize, entry: f32, ray: &Ray, hit: &mut Option<RayHit>) {
        if hit.is_some_and(|best| entry > best.t.abs()) {
            return;
        }
        if node >= self.leaf_start {
            let start = (node - self.leaf_start) * LEAF_TRIANGLES;
            let end = (start + LEAF_TRIANGLES).min(mesh.triangle_count());
            for t in start..end {
                if mesh.hidden[t] {
                    continue;
                }
                let [a, b, c] = mesh.triangles[t].map(|v| mesh.positions[v as usize]);
                if let Some(distance) = line_triangle(ray.origin, ray.direction, a, b, c)
                    && distance.is_finite()
                    && distance != 0.0
                    && hit.is_none_or(|best| {
                        // Favor the forward direction at equal distance,
                        // then make shared-edge hits deterministic.
                        (distance.abs(), distance < 0.0, t as u32) < (best.t.abs(), best.t < 0.0, best.triangle)
                    })
                {
                    *hit = Some(RayHit {
                        t: distance,
                        point: ray.origin + ray.direction * distance,
                        triangle: t as u32,
                        normal: (b - a).cross(c - a).normalize_or_zero(),
                    });
                }
            }
            return;
        }

        let left = node * 2;
        let right = left + 1;
        match (ray.entry(self.nodes[left]), ray.entry(self.nodes[right])) {
            (Some(a), Some(b)) if a <= b => {
                self.visit(mesh, left, a, ray, hit);
                self.visit(mesh, right, b, ray, hit);
            }
            (Some(a), Some(b)) => {
                self.visit(mesh, right, b, ray, hit);
                self.visit(mesh, left, a, ray, hit);
            }
            (Some(a), None) => self.visit(mesh, left, a, ray, hit),
            (None, Some(b)) => self.visit(mesh, right, b, ray, hit),
            (None, None) => {}
        }
    }
}

struct Ray {
    origin: Vec3,
    direction: Vec3,
    inverse: Vec3,
    parallel: glam::BVec3,
}

impl Ray {
    fn entry(&self, b: Aabb) -> Option<f32> {
        if b.is_empty() || ((self.origin.cmplt(b.min) | self.origin.cmpgt(b.max)) & self.parallel).any() {
            return None;
        }
        let a = (b.min - self.origin) * self.inverse;
        let b = (b.max - self.origin) * self.inverse;
        // A ray parallel to a slab and inside it is unconstrained by that
        // axis, including at the exact edge where 0 * infinity is NaN.
        let near = Vec3::select(self.parallel, Vec3::splat(f32::NEG_INFINITY), a.min(b)).max_element();
        let far = Vec3::select(self.parallel, Vec3::splat(f32::INFINITY), a.max(b)).min_element();
        (far >= near).then_some(near.max(-far).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn indexed_rays_match_mesh_hits_and_skip_hidden_triangles() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let hidden: Vec<_> = mesh.triangles.iter().map(|&[a, _, _]| mesh.positions[a as usize].z > 0.0).collect();
        mesh.set_hidden(hidden);
        let index = RaycastIndex::new(&mesh);
        for a in -8..=8 {
            for b in -8..=8 {
                let z = if a % 2 == 0 { 2.0 } else { 0.15 };
                let origin = Vec3::new(a as f32 * 0.13 + 0.001, b as f32 * 0.12 - 0.001, z);
                let dir = -Vec3::Z;
                let regular = [
                    mesh.raycast(origin, dir),
                    mesh.raycast(origin, -dir).map(|mut h| {
                        h.t = -h.t;
                        h
                    }),
                ]
                .into_iter()
                .flatten()
                .min_by(|a, b| a.t.abs().total_cmp(&b.t.abs()));
                let indexed = index.nearest_on_line(&mesh, origin, dir);
                assert_eq!(indexed.map(|h| (h.t, h.triangle)), regular.map(|h| (h.t, h.triangle)));
            }
        }
    }

    #[test]
    fn a_parallel_ray_on_the_edge_of_a_planar_leaf_hits() {
        let mesh = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Z], vec![[0, 1, 2]]);
        let index = RaycastIndex::new(&mesh);
        let hit = index.nearest_on_line(&mesh, Vec3::new(0.0, 1.0, 0.5), Vec3::Y);
        assert_eq!(hit.unwrap().t, -1.0);
        assert!(index.nearest_on_line(&mesh, Vec3::new(-0.001, 1.0, 0.5), -Vec3::Y).is_none());
    }
}
