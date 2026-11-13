//! Generalized winding numbers for triangle soups, to find triangles buried
//! inside the solid a self-intersecting mesh encloses.
//!
//! The winding number of a closed surface at a point is how many times the
//! surface wraps around it: 0 outside, 1 inside, 2 where two parts overlap.
//! It is the sum of each triangle's signed solid angle over 4π (exact solid
//! angle: Van Oosterom & Strackee, "The solid angle of a plane triangle",
//! IEEE Trans. Biomed. Eng. 1983). Far from a cluster of triangles, the
//! cluster's sum is replaced by a dipole (its area-weighted normal at its
//! area-weighted centre), as in Barill, Dickson, Schmidt, Levin & Jacobson,
//! "Fast Winding Numbers for Soups and Clouds", SIGGRAPH 2018, with a
//! bounding volume hierarchy to find the clusters. Only the first-order
//! term is used: it is enough to tell 0 from 1.

use glam::Vec3;
use oxy_mesh::Aabb;
use rayon::prelude::*;

/// Triangles per leaf.
const LEAF: usize = 8;
/// A node counts as far once the query is more than this many node radii away.
const BETA: f32 = 2.0;

struct Node {
    bounds: Aabb,
    /// Area-weighted centre and its distance to the furthest bounds corner.
    centre: Vec3,
    radius: f32,
    /// Sum of area × unit normal (twice the area vector, halved).
    dipole: Vec3,
    /// Children, or a range of `order` for a leaf.
    kind: Kind,
}

enum Kind {
    Inner(u32, u32),
    Leaf(u32, u32),
}

pub struct Winding<'a> {
    positions: &'a [Vec3],
    triangles: &'a [[u32; 3]],
    order: Vec<u32>,
    nodes: Vec<Node>,
}

struct Tris<'a> {
    positions: &'a [Vec3],
    triangles: &'a [[u32; 3]],
    centroid: &'a [Vec3],
    area: &'a [Vec3],
}

/// The subtree for `order` (which starts at `start` in the full order),
/// root first, with each node's total area. Children are listed after
/// their parent; their indices are relative to the returned list. Large
/// halves are built in parallel.
fn build(tri: &Tris, order: &mut [u32], start: usize) -> Vec<(Node, f32)> {
    if order.len() <= LEAF {
        let (mut bounds, mut dipole, mut weighted, mut total) = (Aabb::EMPTY, Vec3::ZERO, Vec3::ZERO, 0.0f32);
        for &t in order.iter() {
            for &v in &tri.triangles[t as usize] {
                let p = tri.positions[v as usize];
                bounds = bounds.union(&Aabb { min: p, max: p });
            }
            let n = tri.area[t as usize];
            let a = n.length();
            dipole += n;
            weighted += tri.centroid[t as usize] * a;
            total += a;
        }
        return vec![(node(bounds, dipole, weighted, total, Kind::Leaf(start as u32, order.len() as u32)), total)];
    }
    let cb = order.iter().fold(Aabb::EMPTY, |b, &t| {
        let c = tri.centroid[t as usize];
        b.union(&Aabb { min: c, max: c })
    });
    let ext = cb.max - cb.min;
    let axis = if ext.x >= ext.y && ext.x >= ext.z {
        0
    } else if ext.y >= ext.z {
        1
    } else {
        2
    };
    let mid = order.len() / 2;
    order.select_nth_unstable_by(mid, |&a, &b| {
        tri.centroid[a as usize][axis].total_cmp(&tri.centroid[b as usize][axis])
    });
    let (lo, hi) = order.split_at_mut(mid);
    let (left, right) = if order_len_is_large(mid) {
        rayon::join(|| build(tri, lo, start), || build(tri, hi, start + mid))
    } else {
        (build(tri, lo, start), build(tri, hi, start + mid))
    };
    let (l, la) = (&left[0].0, left[0].1);
    let (r, ra) = (&right[0].0, right[0].1);
    let bounds = l.bounds.union(&r.bounds);
    let (dipole, weighted, total) = (l.dipole + r.dipole, l.centre * la + r.centre * ra, la + ra);
    let (lc, rc) = (1u32, 1 + left.len() as u32);
    let mut out = Vec::with_capacity(1 + left.len() + right.len());
    out.push((node(bounds, dipole, weighted, total, Kind::Inner(lc, rc)), total));
    for (list, offset) in [(left, lc), (right, rc)] {
        out.extend(list.into_iter().map(|(mut n, a)| {
            if let Kind::Inner(x, y) = n.kind {
                n.kind = Kind::Inner(x + offset, y + offset);
            }
            (n, a)
        }));
    }
    out
}

fn order_len_is_large(n: usize) -> bool {
    n >= 4096
}

fn node(bounds: Aabb, dipole: Vec3, weighted: Vec3, total: f32, kind: Kind) -> Node {
    let centre = if total > 0.0 { weighted / total } else { (bounds.min + bounds.max) * 0.5 };
    let radius = (bounds.min - centre).abs().max((bounds.max - centre).abs()).length();
    Node { bounds, centre, radius, dipole, kind }
}

impl<'a> Winding<'a> {
    pub fn new(positions: &'a [Vec3], triangles: &'a [[u32; 3]]) -> Self {
        let mut order: Vec<u32> = (0..triangles.len() as u32).collect();
        // Per triangle: centroid and half the cross product (area × normal).
        let (centroid, area): (Vec<Vec3>, Vec<Vec3>) = triangles
            .par_iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| positions[v as usize]);
                ((a + b + c) / 3.0, (b - a).cross(c - a) * 0.5)
            })
            .unzip();
        let nodes = if triangles.is_empty() {
            Vec::new()
        } else {
            let tri = Tris { positions, triangles, centroid: &centroid, area: &area };
            build(&tri, &mut order, 0).into_iter().map(|(n, _)| n).collect()
        };
        Winding { positions, triangles, order, nodes }
    }

    fn corners(&self, t: u32) -> [Vec3; 3] {
        self.triangles[t as usize].map(|v| self.positions[v as usize])
    }

    /// The winding number at `q`.
    pub fn at(&self, q: Vec3) -> f32 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0f32;
        // Depth stays under 64: each level halves the triangles.
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let i = stack[top];
            let node = &self.nodes[i as usize];
            let d = node.centre - q;
            let dist = d.length();
            if dist > BETA * node.radius && !node.bounds.contains(q) {
                sum += node.dipole.dot(d) / (dist * dist * dist);
                continue;
            }
            match node.kind {
                Kind::Inner(l, r) => {
                    stack[top] = l;
                    stack[top + 1] = r;
                    top += 2;
                }
                Kind::Leaf(start, len) => {
                    for &t in &self.order[start as usize..(start + len) as usize] {
                        let [a, b, c] = self.corners(t).map(|p| p - q);
                        let (la, lb, lc) = (a.length(), b.length(), c.length());
                        let num = a.dot(b.cross(c));
                        let den = la * lb * lc + a.dot(b) * lc + a.dot(c) * lb + b.dot(c) * la;
                        // Solid angle Ω = 2·atan2(num, den); the sum is Σ Ω / 4π,
                        // and the dipole terms above are already over 4π.
                        sum += 2.0 * num.atan2(den);
                    }
                }
            }
        }
        sum / (4.0 * std::f32::consts::PI)
    }
}

/// Which triangles are buried inside the solid the mesh encloses: the
/// winding number just outside them (a little along their normal) is 1 or
/// more, so another part of the mesh wraps around them. A triangle that
/// crosses another part is judged by its centre.
///
/// RETURNS: one flag per triangle; all false for a mesh that doesn't
/// overlap itself (and for open sheets, whose winding numbers stay below 1).
pub fn buried_triangles(positions: &[Vec3], triangles: &[[u32; 3]]) -> Vec<bool> {
    Winding::new(positions, triangles).buried()
}

impl Winding<'_> {
    /// See [`buried_triangles`].
    pub fn buried(&self) -> Vec<bool> {
        let (positions, triangles) = (self.positions, self.triangles);
        let buried = |t: &[u32; 3]| {
            let [a, b, c] = t.map(|v| positions[v as usize]);
            let n = (b - a).cross(c - a);
            let len = n.length();
            if len <= 0.0 {
                return false;
            }
            let size = (b - a).length().max((c - b).length()).max((a - c).length());
            let q = (a + b + c) / 3.0 + n / len * (size * 1e-3);
            self.at(q) > 0.5
        };
        // Most meshes don't pass through themselves, so first a sample: one
        // triangle in every 32 (at least 4096, so small meshes are checked
        // closely), spread evenly over the surface by taking every k-th
        // leaf. It finds any buried patch bigger than a few dozen triangles;
        // only then is every triangle tested.
        let leaves: Vec<u32> = self
            .nodes
            .iter()
            .filter_map(|n| match n.kind {
                Kind::Leaf(start, len) => Some(self.order[(start + len / 2) as usize]),
                _ => None,
            })
            .collect();
        let wanted = (triangles.len() / 32).max(4096);
        let step = leaves.len().div_ceil(wanted).max(1);
        let samples: Vec<u32> = leaves.into_iter().step_by(step).collect();
        if !samples.par_iter().any(|&t| buried(&triangles[t as usize])) {
            return vec![false; triangles.len()];
        }
        triangles.par_iter().map(buried).collect()
    }
}

/// `flags` grown by `rings` rings of triangles sharing a point.
pub fn grow_triangles(triangles: &[[u32; 3]], flags: &[bool], rings: usize) -> Vec<bool> {
    let mut flags = flags.to_vec();
    let points = triangles.iter().flatten().map(|&v| v as usize + 1).max().unwrap_or(0);
    for _ in 0..rings {
        let mut near = vec![false; points];
        for (t, _) in triangles.iter().zip(&flags).filter(|(_, f)| **f) {
            for &v in t {
                near[v as usize] = true;
            }
        }
        for (t, f) in triangles.iter().zip(flags.iter_mut()) {
            *f |= t.iter().any(|&v| near[v as usize]);
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn closed_sphere_is_one_inside_and_zero_outside() {
        let m = primitives::icosphere(4, 1.0);
        let w = Winding::new(&m.positions, &m.triangles);
        assert!((w.at(Vec3::ZERO) - 1.0).abs() < 0.02);
        assert!((w.at(Vec3::new(0.3, -0.5, 0.2)) - 1.0).abs() < 0.02);
        assert!(w.at(Vec3::new(3.0, 0.0, 0.0)).abs() < 0.02);
        assert!(w.at(Vec3::new(1.05, 0.0, 0.0)).abs() < 0.05);
        assert!(buried_triangles(&m.positions, &m.triangles).iter().all(|b| !b));
    }

    #[test]
    fn the_part_of_a_sphere_inside_another_is_buried() {
        let a = primitives::icosphere(3, 1.0);
        let mut positions = a.positions.clone();
        let n = positions.len() as u32;
        positions.extend(a.positions.iter().map(|p| *p + Vec3::X * 1.2));
        let mut triangles = a.triangles.clone();
        triangles.extend(a.triangles.iter().map(|t| t.map(|i| i + n)));
        let buried = buried_triangles(&positions, &triangles);
        for (t, &b) in triangles.iter().zip(&buried) {
            let c = t.map(|v| positions[v as usize]).iter().sum::<Vec3>() / 3.0;
            let inside_other = if t[0] < n { c.distance(Vec3::X * 1.2) } else { c.length() } < 0.97;
            let outside_other = if t[0] < n { c.distance(Vec3::X * 1.2) } else { c.length() } > 1.03;
            if inside_other {
                assert!(b, "a triangle inside the other sphere is buried");
            }
            if outside_other {
                assert!(!b, "a triangle on the outside is kept");
            }
        }
    }
}
