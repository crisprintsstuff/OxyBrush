//! Shape-preserving decimation by quadric error edge collapse
//! (Garland & Heckbert, "Surface Simplification Using Quadric Error
//! Metrics", 1997).
//!
//! Open boundaries and face-group borders are locked so silhouettes and
//! groups survive. Collapses that would flip a triangle or break the
//! manifold link condition are rejected.

use glam::{DVec3, Vec3};
use oxy_mesh::{Mesh, MeshData};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    fn from_plane(n: DVec3, d: f64, w: f64) -> Self {
        let (a, b, c) = (n.x, n.y, n.z);
        Quadric([a * a, a * b, a * c, a * d, b * b, b * c, b * d, c * c, c * d, d * d].map(|x| x * w))
    }

    fn add(&mut self, o: &Quadric) {
        for i in 0..10 {
            self.0[i] += o.0[i];
        }
    }

    fn error(&self, p: DVec3) -> f64 {
        let q = &self.0;
        let (x, y, z) = (p.x, p.y, p.z);
        q[0] * x * x
            + 2.0 * q[1] * x * y
            + 2.0 * q[2] * x * z
            + 2.0 * q[3] * x
            + q[4] * y * y
            + 2.0 * q[5] * y * z
            + 2.0 * q[6] * y
            + q[7] * z * z
            + 2.0 * q[8] * z
            + q[9]
    }
}

struct Candidate {
    cost: f64,
    a: u32,
    b: u32,
    target: DVec3,
    stamp_a: u32,
    stamp_b: u32,
}

impl PartialEq for Candidate {
    fn eq(&self, o: &Self) -> bool {
        self.cost == o.cost
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Candidate {
    // Min-heap on cost.
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost)
    }
}

/// Reduce `mesh` to about `target_triangles` triangles.
pub fn decimate(mesh: &Mesh, target_triangles: usize) -> Mesh {
    let data = mesh.to_data();
    let mut pos: Vec<DVec3> = data.positions.iter().map(|p| p.as_dvec3()).collect();
    let mut mask = data.mask.clone();
    let mut tris = data.triangles.clone();
    let groups = data.groups.clone();
    let hidden = data.hidden.clone();
    let mut alive = vec![true; tris.len()];
    let mut alive_count = tris.len();
    let vcount = pos.len();

    let mut vt: Vec<Vec<u32>> = vec![Vec::new(); vcount];
    for (t, tri) in tris.iter().enumerate() {
        for &v in tri {
            vt[v as usize].push(t as u32);
        }
    }

    // Lock open-boundary and group-border vertices.
    let mut locked = vec![false; vcount];
    {
        let mut edge_count: std::collections::HashMap<(u32, u32), (u32, u32)> = Default::default();
        for (t, tri) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let e = edge_count.entry((a.min(b), a.max(b))).or_insert((0, groups[t]));
                e.0 += 1;
                if e.1 != groups[t] {
                    locked[a as usize] = true;
                    locked[b as usize] = true;
                }
            }
        }
        for ((a, b), (n, _)) in edge_count {
            if n != 2 {
                locked[a as usize] = true;
                locked[b as usize] = true;
            }
        }
    }

    let mut quadrics = vec![Quadric::default(); vcount];
    for tri in &tris {
        let [a, b, c] = tri.map(|i| pos[i as usize]);
        let n = (b - a).cross(c - a);
        let area = n.length();
        if area <= 0.0 {
            continue;
        }
        let n = n / area;
        let q = Quadric::from_plane(n, -n.dot(a), area);
        for &v in tri {
            quadrics[v as usize].add(&q);
        }
    }

    let mut stamp = vec![0u32; vcount];
    let mut heap = BinaryHeap::new();
    let candidate = |a: u32, b: u32, pos: &[DVec3], quadrics: &[Quadric], locked: &[bool], stamp: &[u32]| {
        if locked[a as usize] && locked[b as usize] {
            return None;
        }
        let mut q = quadrics[a as usize];
        q.add(&quadrics[b as usize]);
        let (pa, pb) = (pos[a as usize], pos[b as usize]);
        let options: &[DVec3] = if locked[a as usize] {
            &[pa]
        } else if locked[b as usize] {
            &[pb]
        } else {
            &[pa, pb, (pa + pb) * 0.5]
        };
        let (cost, target) =
            options.iter().map(|&p| (q.error(p), p)).min_by(|x, y| x.0.total_cmp(&y.0)).expect("non-empty");
        Some(Candidate { cost, a, b, target, stamp_a: stamp[a as usize], stamp_b: stamp[b as usize] })
    };
    for tri in &tris {
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            if a < b
                && let Some(c) = candidate(a, b, &pos, &quadrics, &locked, &stamp)
            {
                heap.push(c);
            }
        }
    }

    let mut neighbors_a = Vec::new();
    let mut neighbors_b = Vec::new();
    while alive_count > target_triangles {
        let Some(c) = heap.pop() else { break };
        let (a, b) = (c.a, c.b);
        if stamp[a as usize] != c.stamp_a || stamp[b as usize] != c.stamp_b {
            continue;
        }
        // Edge must still exist.
        if !vt[a as usize].iter().any(|&t| tris[t as usize].contains(&b)) {
            continue;
        }
        // Link condition: exactly two shared neighbors for an interior edge.
        ring(&vt[a as usize], &tris, a, &mut neighbors_a);
        ring(&vt[b as usize], &tris, b, &mut neighbors_b);
        let shared = neighbors_a.iter().filter(|v| neighbors_b.contains(v)).count();
        if shared != 2 {
            continue;
        }
        // Reject normal flips and slivers among the surviving triangles.
        let flips = |v: u32, other: u32| {
            vt[v as usize].iter().any(|&t| {
                let tri = tris[t as usize];
                if tri.contains(&other) {
                    return false;
                }
                let p = tri.map(|i| pos[i as usize]);
                let before = (p[1] - p[0]).cross(p[2] - p[0]);
                let q = tri.map(|i| if i == v { c.target } else { pos[i as usize] });
                let after = (q[1] - q[0]).cross(q[2] - q[0]);
                before.dot(after) <= 0.0 || after.length_squared() < 1e-6 * before.length_squared()
            })
        };
        if flips(a, b) || flips(b, a) {
            continue;
        }

        // Collapse b into a.
        pos[a as usize] = c.target;
        mask[a as usize] = mask[a as usize].max(mask[b as usize]);
        let qb = quadrics[b as usize];
        quadrics[a as usize].add(&qb);
        locked[a as usize] |= locked[b as usize];
        let b_tris = std::mem::take(&mut vt[b as usize]);
        for t in b_tris {
            if !alive[t as usize] {
                continue;
            }
            let tri = &mut tris[t as usize];
            if tri.contains(&a) {
                alive[t as usize] = false;
                alive_count -= 1;
                for &v in tri.iter() {
                    if v != b {
                        vt[v as usize].retain(|&x| x != t);
                    }
                }
            } else {
                for v in tri.iter_mut() {
                    if *v == b {
                        *v = a;
                    }
                }
                vt[a as usize].push(t);
            }
        }
        // Only edges touching `a` changed cost; other heap entries stay valid.
        stamp[a as usize] += 1;
        stamp[b as usize] += 1;
        ring(&vt[a as usize], &tris, a, &mut neighbors_a);
        for &n in &neighbors_a {
            if let Some(cand) = candidate(a.min(n), a.max(n), &pos, &quadrics, &locked, &stamp) {
                heap.push(cand);
            }
        }
    }

    let mut out =
        MeshData { positions: pos.iter().map(|p| p.as_vec3()).collect::<Vec<Vec3>>(), mask, ..Default::default() };
    for (t, tri) in tris.iter().enumerate() {
        if alive[t] {
            out.triangles.push(*tri);
            out.groups.push(groups[t]);
            out.hidden.push(hidden[t]);
        }
    }
    Mesh::from_data(out)
}

fn ring(vt: &[u32], tris: &[[u32; 3]], v: u32, out: &mut Vec<u32>) {
    out.clear();
    for &t in vt {
        for &u in &tris[t as usize] {
            if u != v && !out.contains(&u) {
                out.push(u);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn sphere_decimates_and_keeps_shape() {
        let s = primitives::icosphere(5, 1.0);
        let d = decimate(&s, 2000);
        assert!(d.triangle_count() <= 2100, "{}", d.triangle_count());
        assert!(d.triangle_count() > 1000);
        for p in &d.positions {
            assert!((p.length() - 1.0).abs() < 0.05, "{}", p.length());
        }
        // Still closed: V - E + F = 2.
        let f = d.triangle_count() as i64;
        assert_eq!(d.vertex_count() as i64 - f * 3 / 2 + f, 2);
    }

    #[test]
    fn plane_boundary_is_preserved() {
        let p = primitives::plane(30, 2.0);
        let d = decimate(&p, 200);
        let b = d.bounds();
        assert!((b.max.x - 1.0).abs() < 1e-5 && (b.min.z + 1.0).abs() < 1e-5);
        assert!(d.triangle_count() < p.triangle_count());
    }

    #[test]
    fn groups_survive() {
        let mut s = primitives::icosphere(4, 1.0);
        let g = s.triangles.iter().map(|t| u32::from(s.positions[t[0] as usize].y > 0.0)).collect();
        s.set_groups(g);
        let d = decimate(&s, 800);
        assert!(d.groups().contains(&0) && d.groups().contains(&1));
    }
}
