//! Chunked triangle mesh for sculpting.
//!
//! Triangles are sorted spatially (Morton order) and split into chunks of
//! roughly [`CHUNK_TRIANGLES`] triangles. Every vertex is *owned* by exactly one
//! chunk, and vertices are renumbered so each chunk owns a contiguous index
//! range. A brush stroke therefore only touches, re-normalizes, re-uploads and
//! snapshots the chunks it actually affects.

mod aabb;
mod adjacency;
pub mod primitives;
pub mod topo;

pub use aabb::Aabb;
pub use glam::Vec3;

use adjacency::Csr;
use rayon::prelude::*;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TOPOLOGY_VERSION: AtomicU64 = AtomicU64::new(1);

/// Target number of triangles per chunk.
pub const CHUNK_TRIANGLES: usize = 4096;

/// A spatially coherent group of triangles plus the vertices it owns.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Vertices owned by this chunk (contiguous in the mesh arrays).
    pub vertices: Range<u32>,
    /// Triangles belonging to this chunk (contiguous in [`Mesh::triangles`]).
    pub triangles: Range<u32>,
    /// Bounds of the chunk's triangles. Always contains its owned vertices.
    pub aabb: Aabb,
}

/// Result of a successful raycast.
#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub t: f32,
    pub point: Vec3,
    pub triangle: u32,
    /// Geometric normal of the hit triangle.
    pub normal: Vec3,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per-vertex mask in `0..=1`; 1 means fully protected from brushes.
    pub mask: Vec<f32>,
    pub triangles: Vec<[u32; 3]>,
    pub chunks: Vec<Chunk>,
    vertex_triangles: Csr,
    vertex_neighbors: Csr,
    vertex_chunk: Vec<u32>,
    triangle_chunk: Vec<u32>,
    dirty: Vec<bool>,
    topology_version: u64,
    tri_stamp: Vec<u32>,
    vert_stamp: Vec<u32>,
    stamp: u32,
}

impl Mesh {
    /// Build a mesh from raw positions and triangles. Unreferenced vertices and
    /// degenerate (repeated-index) triangles are dropped.
    pub fn new(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>) -> Self {
        Self::build(positions, triangles, None)
    }

    fn build(positions: Vec<Vec3>, mut triangles: Vec<[u32; 3]>, mask: Option<Vec<f32>>) -> Self {
        triangles.retain(|t| {
            t[0] != t[1] && t[1] != t[2] && t[0] != t[2] && t.iter().all(|&i| (i as usize) < positions.len())
        });

        // Spatial sort of triangles by centroid Morton code.
        let bounds = Aabb::from_points(positions.iter().copied());
        let extent = bounds.extent().max(Vec3::splat(1e-12));
        let mut keyed: Vec<(u32, [u32; 3])> = triangles
            .iter()
            .map(|t| {
                let c = (positions[t[0] as usize] + positions[t[1] as usize] + positions[t[2] as usize]) / 3.0;
                let n = ((c - bounds.min) / extent).clamp(Vec3::ZERO, Vec3::ONE);
                (morton3(n), *t)
            })
            .collect();
        keyed.sort_unstable_by_key(|k| k.0);
        let mut triangles: Vec<[u32; 3]> = keyed.into_iter().map(|k| k.1).collect();

        // Assign vertex ownership in chunk order and renumber.
        let chunk_count = triangles.len().div_ceil(CHUNK_TRIANGLES).max(1);
        let mut remap = vec![u32::MAX; positions.len()];
        let mut new_positions = Vec::with_capacity(positions.len());
        let mut new_mask = Vec::with_capacity(positions.len());
        let mut chunks = Vec::with_capacity(chunk_count);
        for c in 0..chunk_count {
            let t0 = c * CHUNK_TRIANGLES;
            let t1 = ((c + 1) * CHUNK_TRIANGLES).min(triangles.len());
            let v0 = new_positions.len() as u32;
            for tri in &mut triangles[t0..t1] {
                for i in tri.iter_mut() {
                    let old = *i as usize;
                    if remap[old] == u32::MAX {
                        remap[old] = new_positions.len() as u32;
                        new_positions.push(positions[old]);
                        new_mask.push(mask.as_ref().map_or(0.0, |m| m[old]));
                    }
                    *i = remap[old];
                }
            }
            chunks.push(Chunk {
                vertices: v0..new_positions.len() as u32,
                triangles: t0 as u32..t1 as u32,
                aabb: Aabb::EMPTY,
            });
        }

        let vcount = new_positions.len();
        let mut vertex_chunk = vec![0u32; vcount];
        for (ci, c) in chunks.iter().enumerate() {
            for v in c.vertices.clone() {
                vertex_chunk[v as usize] = ci as u32;
            }
        }
        let mut triangle_chunk = vec![0u32; triangles.len()];
        for (ci, c) in chunks.iter().enumerate() {
            for t in c.triangles.clone() {
                triangle_chunk[t as usize] = ci as u32;
            }
        }

        let vertex_triangles = Csr::vertex_triangles(vcount, &triangles);
        let vertex_neighbors = Csr::vertex_neighbors(vcount, &triangles, &vertex_triangles);

        let tri_count = triangles.len();
        let mut mesh = Mesh {
            normals: vec![Vec3::Z; vcount],
            positions: new_positions,
            mask: new_mask,
            dirty: vec![true; chunks.len()],
            chunks,
            triangles,
            vertex_triangles,
            vertex_neighbors,
            vertex_chunk,
            triangle_chunk,
            topology_version: NEXT_TOPOLOGY_VERSION.fetch_add(1, Ordering::Relaxed),
            tri_stamp: vec![0; tri_count],
            vert_stamp: vec![0; vcount],
            stamp: 0,
        };
        mesh.recompute_all_normals();
        for c in 0..mesh.chunks.len() {
            mesh.refit_chunk(c);
        }
        mesh
    }

    /// Build a mesh carrying a per-vertex mask (indexed like `positions`).
    pub fn with_mask(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>, mask: Vec<f32>) -> Self {
        assert_eq!(positions.len(), mask.len());
        Self::build(positions, triangles, Some(mask))
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Globally unique per topology; renderers rebuild index buffers when it changes.
    pub fn topology_version(&self) -> u64 {
        self.topology_version
    }

    pub fn bounds(&self) -> Aabb {
        self.chunks.iter().fold(Aabb::EMPTY, |a, c| a.union(&c.aabb))
    }

    pub fn neighbors(&self, v: u32) -> &[u32] {
        self.vertex_neighbors.get(v as usize)
    }

    pub fn vertex_triangles(&self, v: u32) -> &[u32] {
        self.vertex_triangles.get(v as usize)
    }

    pub fn vertex_chunk(&self, v: u32) -> u32 {
        self.vertex_chunk[v as usize]
    }

    /// Chunks whose bounds intersect the sphere. Owned vertices of other chunks
    /// are guaranteed to lie outside it.
    pub fn chunks_in_sphere(&self, center: Vec3, radius: f32) -> Vec<u32> {
        self.chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.aabb.distance_sq(center) <= radius * radius)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Mark a chunk as needing GPU re-upload.
    pub fn mark_dirty(&mut self, chunk: u32) {
        self.dirty[chunk as usize] = true;
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    /// Returns and clears the set of dirty chunks.
    pub fn take_dirty(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, d) in self.dirty.iter_mut().enumerate() {
            if *d {
                out.push(i as u32);
                *d = false;
            }
        }
        out
    }

    /// Recompute normals and bounds after the given vertices moved, and mark
    /// every affected chunk dirty.
    pub fn update_after_move(&mut self, moved: &[u32]) {
        if moved.is_empty() {
            return;
        }
        // Generation stamps dedup triangles/vertices in O(n) without sorting.
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.tri_stamp.iter_mut().for_each(|x| *x = 0);
            self.vert_stamp.iter_mut().for_each(|x| *x = 0);
            self.stamp = 1;
        }
        let stamp = self.stamp;
        let mut tris = Vec::with_capacity(moved.len() * 6);
        for &v in moved {
            for &t in self.vertex_triangles.get(v as usize) {
                if self.tri_stamp[t as usize] != stamp {
                    self.tri_stamp[t as usize] = stamp;
                    tris.push(t);
                }
            }
        }
        let mut verts = Vec::with_capacity(tris.len() / 2);
        for &t in &tris {
            for v in self.triangles[t as usize] {
                if self.vert_stamp[v as usize] != stamp {
                    self.vert_stamp[v as usize] = stamp;
                    verts.push(v);
                }
            }
        }
        let normals: Vec<Vec3> = verts.par_iter().map(|&v| self.compute_vertex_normal(v)).collect();
        for (&v, n) in verts.iter().zip(normals) {
            self.normals[v as usize] = n;
            self.dirty[self.vertex_chunk[v as usize] as usize] = true;
        }

        let mut chunk_seen = vec![false; self.chunks.len()];
        let mut chunks = Vec::new();
        for &t in &tris {
            let c = self.triangle_chunk[t as usize];
            if !std::mem::replace(&mut chunk_seen[c as usize], true) {
                chunks.push(c);
            }
        }
        let boxes: Vec<Aabb> = chunks.par_iter().map(|&c| self.chunk_bounds(c as usize)).collect();
        for (c, b) in chunks.into_iter().zip(boxes) {
            self.chunks[c as usize].aabb = b;
        }
    }

    /// Recompute normals and bounds for whole chunks (e.g. after undo).
    pub fn update_chunks(&mut self, chunks: &[u32]) {
        let moved: Vec<u32> = chunks.iter().flat_map(|&c| self.chunks[c as usize].vertices.clone()).collect();
        for &c in chunks {
            self.dirty[c as usize] = true;
        }
        self.update_after_move(&moved);
    }

    /// Recompute all normals and bounds and mark everything dirty, after
    /// positions were edited directly.
    pub fn refresh_all(&mut self) {
        self.recompute_all_normals();
        for c in 0..self.chunks.len() {
            self.refit_chunk(c);
        }
        self.mark_all_dirty();
    }

    pub fn recompute_all_normals(&mut self) {
        let normals: Vec<Vec3> =
            (0..self.positions.len() as u32).into_par_iter().map(|v| self.compute_vertex_normal(v)).collect();
        self.normals = normals;
    }

    fn compute_vertex_normal(&self, v: u32) -> Vec3 {
        let mut n = Vec3::ZERO;
        for &t in self.vertex_triangles.get(v as usize) {
            // Unnormalized cross product = area-weighted face normal.
            n += self.face_normal_unnormalized(t);
        }
        n.try_normalize().unwrap_or(Vec3::Z)
    }

    pub fn face_normal_unnormalized(&self, t: u32) -> Vec3 {
        let [a, b, c] = self.triangles[t as usize];
        let (a, b, c) = (self.positions[a as usize], self.positions[b as usize], self.positions[c as usize]);
        (b - a).cross(c - a)
    }

    fn refit_chunk(&mut self, c: usize) {
        self.chunks[c].aabb = self.chunk_bounds(c);
    }

    fn chunk_bounds(&self, c: usize) -> Aabb {
        let range = self.chunks[c].triangles.clone();
        let mut aabb = Aabb::EMPTY;
        for t in &self.triangles[range.start as usize..range.end as usize] {
            for &i in t {
                aabb.grow(self.positions[i as usize]);
            }
        }
        aabb
    }

    /// Closest intersection of a ray with the mesh (front and back faces).
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<RayHit> {
        let inv = dir.recip();
        let mut candidates: Vec<(f32, usize)> =
            self.chunks.iter().enumerate().filter_map(|(i, c)| c.aabb.ray_entry(origin, inv).map(|t| (t, i))).collect();
        candidates.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

        let mut best: Option<RayHit> = None;
        for (tmin, ci) in candidates {
            if best.is_some_and(|b| tmin > b.t) {
                break;
            }
            let r = self.chunks[ci].triangles.clone();
            for ti in r {
                let [a, b, c] = self.triangles[ti as usize];
                let (a, b, c) = (self.positions[a as usize], self.positions[b as usize], self.positions[c as usize]);
                if let Some(t) = ray_triangle(origin, dir, a, b, c)
                    && best.is_none_or(|h| t < h.t)
                {
                    best = Some(RayHit {
                        t,
                        point: origin + dir * t,
                        triangle: ti,
                        normal: (b - a).cross(c - a).normalize_or_zero(),
                    });
                }
            }
        }
        best
    }
}

/// Möller–Trumbore ray/triangle intersection, two-sided.
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

fn morton3(n: Vec3) -> u32 {
    fn spread(mut x: u32) -> u32 {
        x &= 0x3ff;
        x = (x | (x << 16)) & 0x030000ff;
        x = (x | (x << 8)) & 0x0300f00f;
        x = (x | (x << 4)) & 0x030c30c3;
        x = (x | (x << 2)) & 0x09249249;
        x
    }
    let q = (n * 1023.0).as_uvec3();
    spread(q.x) | (spread(q.y) << 1) | (spread(q.z) << 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_own_contiguous_vertices() {
        let m = primitives::icosphere(5, 1.0);
        let mut next = 0;
        for c in &m.chunks {
            assert_eq!(c.vertices.start, next);
            next = c.vertices.end;
            for v in c.vertices.clone() {
                assert!(c.aabb.contains(m.positions[v as usize]));
            }
        }
        assert_eq!(next as usize, m.vertex_count());
        assert!(m.chunks.len() > 1);
    }

    #[test]
    fn sphere_normals_point_outward() {
        let m = primitives::icosphere(3, 2.0);
        for (p, n) in m.positions.iter().zip(&m.normals) {
            assert!(p.normalize().dot(*n) > 0.99);
        }
    }

    #[test]
    fn raycast_hits_front_of_sphere() {
        let m = primitives::icosphere(4, 1.0);
        let hit = m.raycast(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z).unwrap();
        assert!((hit.t - 4.0).abs() < 0.01, "t = {}", hit.t);
        assert!(m.raycast(Vec3::new(5.0, 5.0, 5.0), Vec3::Z).is_none());
    }

    #[test]
    fn update_after_move_marks_dirty_and_refits() {
        let mut m = primitives::icosphere(5, 1.0);
        m.take_dirty();
        let v = 0u32;
        m.positions[v as usize] *= 3.0;
        m.update_after_move(&[v]);
        let dirty = m.take_dirty();
        assert!(dirty.contains(&m.vertex_chunk(v)));
        assert!(m.bounds().max.length() > 2.0 || m.bounds().min.length() > 2.0);
    }
}
