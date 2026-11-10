//! Shape-preserving decimation by quadric error edge collapse
//! (Garland & Heckbert, "Surface Simplification Using Quadric Error
//! Metrics", 1997).
//!
//! Open boundaries and face-group borders are locked so silhouettes and
//! groups survive. Collapses that would flip a triangle or break the
//! manifold link condition are rejected.

use glam::{DVec3, Vec3};
use oxy_mesh::{Mesh, MeshData};
use rayon::prelude::*;

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

/// A queued edge collapse (the payload; its cost lives in the queue key).
#[derive(Clone, Copy)]
struct Candidate {
    a: u32,
    b: u32,
    stamp_a: u32,
    stamp_b: u32,
}

/// Min-priority queue of edge collapses: a 4-ary heap of packed u64 keys
/// (cost bits in the high half, payload index in the low half).
///
/// Most queued collapses go stale before they are popped (a collapse
/// re-queues every edge around the merged vertex), so the queue sees about
/// ten pops per accepted collapse. Small integer keys and a shallow heap
/// keep those pops cache-friendly; this queue was ~60% of decimation time
/// with `BinaryHeap<48-byte struct>`.
struct Queue {
    heap: Vec<u64>,
    payload: Vec<Candidate>,
}

impl Queue {
    fn with_capacity(n: usize) -> Self {
        Self { heap: Vec::with_capacity(n), payload: Vec::with_capacity(n) }
    }

    /// f32 bits mapped so that unsigned integer order equals float order.
    fn ordered_bits(cost: f64) -> u64 {
        let bits = (cost as f32).to_bits();
        (if bits >> 31 == 1 { !bits } else { bits | 0x8000_0000 }) as u64
    }

    fn key(&mut self, cost: f64, c: Candidate) -> u64 {
        let index = self.payload.len() as u64;
        assert!(index < u32::MAX as u64, "too many queued collapses");
        self.payload.push(c);
        (Self::ordered_bits(cost) << 32) | index
    }

    /// Build from unordered entries in O(n).
    fn heapify(&mut self) {
        let n = self.heap.len();
        for i in (0..n.div_ceil(4) + 1).rev() {
            self.sift_down(i);
        }
    }

    fn push(&mut self, cost: f64, c: Candidate) {
        let k = self.key(cost, c);
        self.heap.push(k);
        let mut i = self.heap.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 4;
            if self.heap[parent] <= k {
                break;
            }
            self.heap[i] = self.heap[parent];
            i = parent;
        }
        self.heap[i] = k;
    }

    fn pop(&mut self) -> Option<Candidate> {
        let top = *self.heap.first()?;
        let last = self.heap.pop().expect("non-empty");
        if !self.heap.is_empty() {
            self.heap[0] = last;
            self.sift_down(0);
        }
        Some(self.payload[(top & 0xFFFF_FFFF) as usize])
    }

    fn sift_down(&mut self, mut i: usize) {
        let n = self.heap.len();
        if i >= n {
            return;
        }
        let k = self.heap[i];
        loop {
            let first = 4 * i + 1;
            if first >= n {
                break;
            }
            let last = (first + 4).min(n);
            let (mut best, mut best_key) = (first, self.heap[first]);
            for c in first + 1..last {
                if self.heap[c] < best_key {
                    (best, best_key) = (c, self.heap[c]);
                }
            }
            if best_key >= k {
                break;
            }
            self.heap[i] = best_key;
            i = best;
        }
        self.heap[i] = k;
    }
}

/// The mesh being decimated (or one part of it).
struct Work {
    pos: Vec<DVec3>,
    mask: Vec<f32>,
    tris: Vec<[u32; 3]>,
    groups: Vec<u32>,
}

/// What a part hands back: its triangles (in the whole mesh's vertex
/// numbers), their groups and hidden flags, and its own vertices' new
/// positions and mask.
type PartResult = (Vec<[u32; 3]>, Vec<u32>, Vec<bool>, Vec<(u32, DVec3, f32)>);

/// Meshes larger than this are decimated in parallel parts first.
const PARALLEL_ABOVE: usize = 200_000;

/// Reduce `mesh` to about `target_triangles` triangles.
///
/// Large meshes are cut into spatially compact parts (runs of the mesh's
/// chunks, which are ordered along a space-filling curve) that are
/// decimated in parallel with the vertices they share locked, so no
/// cracks open; a last pass over the whole mesh, with nothing extra
/// locked, then reaches the target, mostly along the part borders.
pub fn decimate(mesh: &Mesh, target_triangles: usize) -> Mesh {
    decimate_with(mesh, target_triangles, true)
}

fn decimate_with(mesh: &Mesh, target_triangles: usize, allow_parallel: bool) -> Mesh {
    let data = mesh.to_data();
    let vcount = data.positions.len();
    let n = data.triangles.len();
    let mut w = Work {
        pos: data.positions.iter().map(|p| p.as_dvec3()).collect(),
        mask: data.mask.clone(),
        tris: data.triangles.clone(),
        groups: data.groups.clone(),
    };
    let mut hidden = data.hidden.clone();

    let threads = rayon::current_num_threads();
    if allow_parallel && n > PARALLEL_ABOVE && target_triangles < n && threads > 1 && mesh.chunks.len() > 1 {
        let parts = (threads * 4).min(mesh.chunks.len());
        let per = mesh.chunks.len().div_ceil(parts);
        let ranges: Vec<std::ops::Range<usize>> = mesh
            .chunks
            .chunks(per)
            .map(|cs| cs[0].triangles.start as usize..cs[cs.len() - 1].triangles.end as usize)
            .collect();
        // Vertices used by more than one part stay where they are.
        let mut owner = vec![u32::MAX; vcount];
        let mut shared = vec![false; vcount];
        for (pi, r) in ranges.iter().enumerate() {
            for tri in &w.tris[r.clone()] {
                for &v in tri {
                    let o = &mut owner[v as usize];
                    if *o == u32::MAX {
                        *o = pi as u32;
                    } else if *o != pi as u32 {
                        shared[v as usize] = true;
                    }
                }
            }
        }
        let ratio = target_triangles as f64 / n as f64;
        let (w_ref, shared) = (&w, &shared);
        let results: Vec<PartResult> = ranges
            .par_iter()
            .map(|r| {
                // The part with its own vertex numbering.
                let mut local_of = vec![u32::MAX; vcount];
                let mut globals: Vec<u32> = Vec::new();
                let tris: Vec<[u32; 3]> = w_ref.tris[r.clone()]
                    .iter()
                    .map(|tri| {
                        tri.map(|v| {
                            let l = &mut local_of[v as usize];
                            if *l == u32::MAX {
                                *l = globals.len() as u32;
                                globals.push(v);
                            }
                            *l
                        })
                    })
                    .collect();
                let mut part = Work {
                    pos: globals.iter().map(|&v| w_ref.pos[v as usize]).collect(),
                    mask: globals.iter().map(|&v| w_ref.mask.get(v as usize).copied().unwrap_or(0.0)).collect(),
                    tris,
                    groups: w_ref.groups[r.clone()].to_vec(),
                };
                let pinned: Vec<bool> = globals.iter().map(|&v| shared[v as usize]).collect();
                let target = ((r.len() as f64) * ratio) as usize;
                let alive = collapse(&mut part, &pinned, target);
                let mut tris = Vec::new();
                let mut groups = Vec::new();
                let mut part_hidden = Vec::new();
                for (k, tri) in part.tris.iter().enumerate() {
                    if alive[k] {
                        tris.push(tri.map(|l| globals[l as usize]));
                        groups.push(part.groups[k]);
                        part_hidden.push(hidden[r.start + k]);
                    }
                }
                let moved: Vec<(u32, DVec3, f32)> = globals
                    .iter()
                    .enumerate()
                    .filter(|&(_, &g)| !shared[g as usize])
                    .map(|(l, &g)| (g, part.pos[l], part.mask[l]))
                    .collect();
                (tris, groups, part_hidden, moved)
            })
            .collect();
        w.tris.clear();
        w.groups.clear();
        hidden.clear();
        for (tris, groups, part_hidden, moved) in results {
            w.tris.extend(tris);
            w.groups.extend(groups);
            hidden.extend(part_hidden);
            for (g, p, m) in moved {
                w.pos[g as usize] = p;
                if let Some(x) = w.mask.get_mut(g as usize) {
                    *x = m;
                }
            }
        }
    }

    let nothing_pinned = vec![false; w.pos.len()];
    let alive = collapse(&mut w, &nothing_pinned, target_triangles);
    let mut out = MeshData {
        positions: w.pos.iter().map(|p| p.as_vec3()).collect::<Vec<Vec3>>(),
        mask: w.mask,
        ..Default::default()
    };
    for (t, tri) in w.tris.iter().enumerate() {
        if alive[t] {
            out.triangles.push(*tri);
            out.groups.push(w.groups[t]);
            out.hidden.push(hidden[t]);
        }
    }
    Mesh::from_data(out)
}

/// Collapse edges of `w` (cheapest first) until at most `target`
/// triangles are left or nothing more can go. No edge touching a `pinned`
/// vertex collapses; open boundaries and face-group borders don't move.
/// Returns which triangles are left.
fn collapse(w: &mut Work, pinned: &[bool], target: usize) -> Vec<bool> {
    let Work { pos, mask, tris, groups } = w;
    let groups: &[u32] = groups;
    let vcount = pos.len();
    let mut alive = vec![true; tris.len()];
    let mut alive_count = tris.len();
    if mask.len() != vcount {
        mask.resize(vcount, 0.0);
    }
    let target_triangles = target;

    let mut vt: Vec<Vec<u32>> = vec![Vec::new(); vcount];
    for (t, tri) in tris.iter().enumerate() {
        for &v in tri {
            vt[v as usize].push(t as u32);
        }
    }

    // Lock open-boundary and group-border vertices.
    let mut locked = pinned.to_vec();
    {
        // Sort the half-edges by edge so each edge's triangles are adjacent:
        // an edge not shared by exactly two triangles is an open boundary
        // (or non-manifold), and one whose triangles differ in face group is
        // a group border.
        let mut he: Vec<(u64, u32)> = (0..tris.len() * 3)
            .into_par_iter()
            .map(|o| {
                let tri = tris[o / 3];
                let (a, b) = (tri[o % 3], tri[(o + 1) % 3]);
                (((a.min(b) as u64) << 32) | a.max(b) as u64, groups[o / 3])
            })
            .collect();
        he.par_sort_unstable();
        let mut i = 0;
        while i < he.len() {
            let mut j = i + 1;
            while j < he.len() && he[j].0 == he[i].0 {
                j += 1;
            }
            if j - i != 2 || he[i].1 != he[j - 1].1 {
                let (a, b) = ((he[i].0 >> 32) as usize, he[i].0 as u32 as usize);
                locked[a] = true;
                locked[b] = true;
            }
            i = j;
        }
    }

    let mut quadrics = vec![Quadric::default(); vcount];
    for tri in tris.iter() {
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
    // Best collapse position and its error for edge (a, b): a locked vertex
    // stays put; otherwise the cheaper of the endpoints and the midpoint.
    let best = |a: u32, b: u32, pos: &[DVec3], quadrics: &[Quadric], locked: &[bool]| -> Option<(f64, DVec3)> {
        // A pinned vertex is shared with another part of the mesh: this part
        // can't see all its triangles, so no edge touching it collapses here
        // (the whole-mesh pass afterwards takes care of them).
        if (locked[a as usize] && locked[b as usize]) || pinned[a as usize] || pinned[b as usize] {
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
        options.iter().map(|&p| (q.error(p), p)).min_by(|x, y| x.0.total_cmp(&y.0))
    };
    let candidate = |a: u32, b: u32, pos: &[DVec3], quadrics: &[Quadric], locked: &[bool], stamp: &[u32]| {
        best(a, b, pos, quadrics, locked)
            .map(|(cost, _)| (cost, Candidate { a, b, stamp_a: stamp[a as usize], stamp_b: stamp[b as usize] }))
    };
    // Initial queue: every edge once (from the triangle where a < b),
    // computed in parallel and heapified in one O(n) pass.
    let initial: Vec<(f64, Candidate)> = tris
        .par_iter()
        .flat_map_iter(|tri| {
            let (pos, quadrics, locked, stamp) = (&pos, &quadrics, &locked, &stamp);
            (0..3).filter_map(move |k| {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                if a < b { candidate(a, b, pos, quadrics, locked, stamp) } else { None }
            })
        })
        .collect();
    let mut heap = Queue::with_capacity(initial.len() * 3);
    for (cost, c) in initial {
        let k = heap.key(cost, c);
        heap.heap.push(k);
    }
    heap.heapify();

    let mut neighbors_a = Vec::new();
    let mut neighbors_b = Vec::new();
    while alive_count > target_triangles {
        let Some(c) = heap.pop() else { break };
        let (mut a, mut b) = (c.a, c.b);
        if stamp[a as usize] != c.stamp_a || stamp[b as usize] != c.stamp_b {
            continue;
        }
        // The locked end of an edge (an open boundary or group border) stays
        // where it is and keeps its index.
        if locked[b as usize] && !locked[a as usize] {
            std::mem::swap(&mut a, &mut b);
        }
        // Same inputs as when queued (stamps match), so the same result.
        let Some((_, target)) = best(a, b, pos, &quadrics, &locked) else { continue };
        // Edge must still exist.
        if !vt[a as usize].iter().any(|&t| tris[t as usize].contains(&b)) {
            continue;
        }
        // Link condition: exactly two shared neighbors for an interior edge.
        ring(&vt[a as usize], tris, a, &mut neighbors_a);
        ring(&vt[b as usize], tris, b, &mut neighbors_b);
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
                let q = tri.map(|i| if i == v { target } else { pos[i as usize] });
                let after = (q[1] - q[0]).cross(q[2] - q[0]);
                before.dot(after) <= 0.0 || after.length_squared() < 1e-6 * before.length_squared()
            })
        };
        if flips(a, b) || flips(b, a) {
            continue;
        }

        // Collapse b into a.
        pos[a as usize] = target;
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
        ring(&vt[a as usize], tris, a, &mut neighbors_a);
        for &n in &neighbors_a {
            if let Some((cost, cand)) = candidate(a.min(n), a.max(n), pos, &quadrics, &locked, &stamp) {
                heap.push(cost, cand);
            }
        }
    }

    alive
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
    fn queue_pops_in_cost_order() {
        let mut q = Queue::with_capacity(0);
        let costs = [3.0, -1.0, 0.5, 1e-9, 2.0, 0.0, 7.5, -0.25, 1e6, 0.5];
        for (i, &c) in costs.iter().enumerate() {
            let cand = Candidate { a: i as u32, b: 0, stamp_a: 0, stamp_b: 0 };
            if i < 5 {
                q.push(c, cand);
            } else {
                // The rest go in unordered and are heapified, as at startup.
                let k = q.key(c, cand);
                q.heap.push(k);
            }
        }
        q.heapify();
        let mut popped = Vec::new();
        while let Some(c) = q.pop() {
            popped.push(costs[c.a as usize]);
        }
        let mut sorted = costs.to_vec();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(popped, sorted);
    }

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
    fn large_meshes_decimate_in_parallel_without_cracks() {
        let s = primitives::icosphere(7, 1.0);
        assert!(s.triangle_count() > PARALLEL_ABOVE);
        let target = 20_000;
        let mean_error =
            |m: &Mesh| m.positions.iter().map(|p| (p.length() - 1.0).abs()).sum::<f32>() / m.vertex_count() as f32;
        let parallel = decimate(&s, target);
        let serial = decimate_with(&s, target, false);
        let f = parallel.triangle_count();
        assert!(f <= target + target / 20 && f > target * 9 / 10, "{f}");
        // Closed, with every edge shared by exactly two triangles: no
        // cracks where the parts met.
        let mut edges = std::collections::HashMap::new();
        for t in &parallel.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&c| c == 2), "open or non-manifold edges after the parallel pass");
        assert_eq!(parallel.vertex_count() as i64 - edges.len() as i64 + f as i64, 2);
        // As good a shape as decimating it all at once.
        let (ep, es) = (mean_error(&parallel), mean_error(&serial));
        assert!(ep < es * 1.5 + 1e-5, "parallel {ep} vs serial {es}");
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
