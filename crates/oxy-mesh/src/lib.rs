//! Chunked triangle mesh for sculpting.
//!
//! Triangles are sorted spatially (Morton order) and split into chunks of
//! roughly [`CHUNK_TRIANGLES`] triangles. Every vertex is *owned* by exactly one
//! chunk, and vertices are renumbered so each chunk owns a contiguous index
//! range. A brush stroke therefore only touches, re-normalizes, re-uploads and
//! snapshots the chunks it actually affects.

mod aabb;
mod adjacency;
pub mod multires;

/// Per-triangle data carried through the spatial sort: face group, hidden, source index.
type TriAttr = (u32, bool, u32);
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

/// Plain mesh data, e.g. for file I/O or topology operations. Optional
/// attribute vectors may be empty (meaning default values).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Per vertex.
    pub mask: Vec<f32>,
    /// Per triangle face group id.
    pub groups: Vec<u32>,
    /// Per triangle visibility.
    pub hidden: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per-vertex mask in `0..=1`; 1 means fully protected from brushes.
    pub mask: Vec<f32>,
    pub triangles: Vec<[u32; 3]>,
    pub chunks: Vec<Chunk>,
    /// Per-triangle face group id.
    groups: Vec<u32>,
    /// Per-triangle hidden flag.
    hidden: Vec<bool>,
    /// A vertex is hidden when all of its triangles are hidden.
    vertex_hidden: Vec<bool>,
    face_version: u64,
    /// Index of each vertex / triangle in the `MeshData` it was built from
    /// (construction reorders both for locality).
    source_vertex: Vec<u32>,
    source_triangle: Vec<u32>,
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
        Self::from_data(MeshData { positions, triangles, ..Default::default() })
    }

    /// Build from [`MeshData`]. Triangles and vertices are reordered for
    /// locality; per-triangle and per-vertex attributes follow them.
    pub fn from_data(data: MeshData) -> Self {
        let MeshData { positions, triangles, mask, groups, hidden } = data;
        let mask = (mask.len() == positions.len()).then_some(mask);
        let tri_attr = |i: usize| (groups.get(i).copied().unwrap_or(0), hidden.get(i).copied().unwrap_or(false));
        let triangles: Vec<([u32; 3], (u32, bool, u32))> = triangles
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let (g, h) = tri_attr(i);
                (t, (g, h, i as u32))
            })
            .filter(|(t, _)| {
                t[0] != t[1] && t[1] != t[2] && t[0] != t[2] && t.iter().all(|&i| (i as usize) < positions.len())
            })
            .collect();

        // Spatial sort of triangles by centroid Morton code.
        let bounds = Aabb::from_points(positions.iter().copied());
        let extent = bounds.extent().max(Vec3::splat(1e-12));
        // (morton key, triangle, (face group, hidden, source triangle index))
        let mut keyed: Vec<(u32, [u32; 3], TriAttr)> = triangles
            .into_iter()
            .map(|(t, a)| {
                let c = (positions[t[0] as usize] + positions[t[1] as usize] + positions[t[2] as usize]) / 3.0;
                let n = ((c - bounds.min) / extent).clamp(Vec3::ZERO, Vec3::ONE);
                (morton3(n), t, a)
            })
            .collect();
        keyed.sort_unstable_by_key(|k| k.0);
        let mut triangles = Vec::with_capacity(keyed.len());
        let mut groups = Vec::with_capacity(keyed.len());
        let mut hidden = Vec::with_capacity(keyed.len());
        let mut source_triangle = Vec::with_capacity(keyed.len());
        for (_, t, (g, h, src)) in keyed {
            triangles.push(t);
            groups.push(g);
            hidden.push(h);
            source_triangle.push(src);
        }

        // Assign vertex ownership in chunk order and renumber.
        let chunk_count = triangles.len().div_ceil(CHUNK_TRIANGLES).max(1);
        let mut remap = vec![u32::MAX; positions.len()];
        let mut new_positions = Vec::with_capacity(positions.len());
        let mut new_mask = Vec::with_capacity(positions.len());
        let mut source_vertex = Vec::with_capacity(positions.len());
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
                        source_vertex.push(old as u32);
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
            groups,
            hidden,
            vertex_hidden: Vec::new(),
            face_version: 0,
            source_vertex,
            source_triangle,
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
        mesh.faces_changed();
        mesh
    }

    /// Build a mesh carrying a per-vertex mask (indexed like `positions`).
    pub fn with_mask(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>, mask: Vec<f32>) -> Self {
        assert_eq!(positions.len(), mask.len());
        Self::from_data(MeshData { positions, triangles, mask, ..Default::default() })
    }

    /// Copy out plain data (in this mesh's vertex/triangle order).
    pub fn to_data(&self) -> MeshData {
        MeshData {
            positions: self.positions.clone(),
            triangles: self.triangles.clone(),
            mask: self.mask.clone(),
            groups: self.groups.clone(),
            hidden: self.hidden.clone(),
        }
    }

    // ---- mask operations (all respect hidden vertices: they are left alone) ----

    /// Average each visible vertex's mask with its neighbors' (`passes` times).
    pub fn blur_mask(&mut self, passes: u32) {
        for _ in 0..passes {
            let prev = self.mask.clone();
            for v in 0..self.positions.len() {
                if self.vertex_hidden[v] {
                    continue;
                }
                let n = self.vertex_neighbors.get(v);
                if !n.is_empty() {
                    let avg = n.iter().map(|&i| prev[i as usize]).sum::<f32>() / n.len() as f32;
                    self.mask[v] = (prev[v] + avg) * 0.5;
                }
            }
        }
    }

    /// Push mask values toward 0 or 1 (inverse of blur).
    pub fn sharpen_mask(&mut self) {
        for (m, &h) in self.mask.iter_mut().zip(&self.vertex_hidden) {
            if !h {
                *m = ((*m - 0.5) * 2.0 + 0.5).clamp(0.0, 1.0);
            }
        }
    }

    /// Grow (`grow = true`) or shrink the masked area by one ring of vertices.
    pub fn grow_mask(&mut self, grow: bool) {
        let prev = self.mask.clone();
        for v in 0..self.positions.len() {
            if self.vertex_hidden[v] {
                continue;
            }
            let n = self.vertex_neighbors.get(v).iter().map(|&i| prev[i as usize]);
            self.mask[v] = if grow { n.fold(prev[v], f32::max) } else { n.fold(prev[v], f32::min) };
        }
    }

    /// Mask by surface curvature: `cavities = true` masks concave areas
    /// (creases, pores), otherwise convex peaks. Strength is normalised so
    /// the strongest 5% of the mesh reaches a full mask.
    pub fn mask_by_curvature(&mut self, cavities: bool) {
        // Signed mean-curvature proxy: how far the neighbor average lies
        // above (concave) or below (convex) the vertex along its normal,
        // relative to the local edge length.
        let curv: Vec<f32> = (0..self.positions.len())
            .map(|v| {
                let n = self.vertex_neighbors.get(v);
                if n.is_empty() {
                    return 0.0;
                }
                let p = self.positions[v];
                let avg = n.iter().map(|&i| self.positions[i as usize]).sum::<Vec3>() / n.len() as f32;
                let edge = n.iter().map(|&i| self.positions[i as usize].distance(p)).sum::<f32>() / n.len() as f32;
                let c = (avg - p).dot(self.normals[v]) / edge.max(1e-12);
                if cavities { c } else { -c }
            })
            .collect();
        let mut sorted: Vec<f32> = curv.iter().copied().filter(|&c| c > 0.0).collect();
        sorted.sort_unstable_by(f32::total_cmp);
        let scale = sorted.get(sorted.len().saturating_sub(1) * 95 / 100).copied().unwrap_or(1.0).max(1e-6);
        for (v, c) in curv.into_iter().enumerate() {
            if !self.vertex_hidden[v] {
                self.mask[v] = (c / scale).clamp(0.0, 1.0);
            }
        }
    }

    /// Fully mask every vertex of face group `group`; unmask the rest.
    pub fn mask_group(&mut self, group: u32) {
        let mut in_group = vec![false; self.positions.len()];
        for (t, tri) in self.triangles.iter().enumerate() {
            if self.groups[t] == group {
                for &v in tri {
                    in_group[v as usize] = true;
                }
            }
        }
        for (v, &g) in in_group.iter().enumerate() {
            if !self.vertex_hidden[v] {
                self.mask[v] = if g { 1.0 } else { 0.0 };
            }
        }
    }

    // ---- face groups and visibility ----

    /// Grow the visible area by one ring of triangles (`grow = true`) or
    /// shrink it by hiding triangles on its border.
    pub fn grow_visible(&mut self, grow: bool) {
        let n = self.positions.len();
        // Vertices touching at least one visible / at least one hidden triangle.
        let mut touches_visible = vec![false; n];
        let mut touches_hidden = vec![false; n];
        for (t, tri) in self.triangles.iter().enumerate() {
            for &v in tri {
                if self.hidden[t] {
                    touches_hidden[v as usize] = true;
                } else {
                    touches_visible[v as usize] = true;
                }
            }
        }
        let hidden = self
            .triangles
            .iter()
            .zip(&self.hidden)
            .map(|(tri, &h)| {
                if grow {
                    h && !tri.iter().any(|&v| touches_visible[v as usize])
                } else {
                    h || tri.iter().any(|&v| touches_hidden[v as usize])
                }
            })
            .collect();
        self.set_hidden(hidden);
    }

    /// For each vertex, its index in the `MeshData` this mesh was built from.
    pub fn source_vertices(&self) -> &[u32] {
        &self.source_vertex
    }

    /// For each triangle, its index in the `MeshData` this mesh was built from.
    pub fn source_triangles(&self) -> &[u32] {
        &self.source_triangle
    }

    /// Changes whenever face groups or visibility change (globally unique).
    pub fn face_version(&self) -> u64 {
        self.face_version
    }

    fn faces_changed(&mut self) {
        self.vertex_hidden = (0..self.positions.len())
            .map(|v| {
                let ts = self.vertex_triangles.get(v);
                !ts.is_empty() && ts.iter().all(|&t| self.hidden[t as usize])
            })
            .collect();
        self.face_version = NEXT_TOPOLOGY_VERSION.fetch_add(1, Ordering::Relaxed);
    }

    pub fn groups(&self) -> &[u32] {
        &self.groups
    }

    pub fn hidden(&self) -> &[bool] {
        &self.hidden
    }

    pub fn is_vertex_hidden(&self, v: u32) -> bool {
        self.vertex_hidden[v as usize]
    }

    pub fn any_hidden(&self) -> bool {
        self.hidden.iter().any(|&h| h)
    }

    pub fn set_groups(&mut self, groups: Vec<u32>) {
        assert_eq!(groups.len(), self.triangles.len());
        self.groups = groups;
        self.faces_changed();
    }

    pub fn set_hidden(&mut self, hidden: Vec<bool>) {
        assert_eq!(hidden.len(), self.triangles.len());
        self.hidden = hidden;
        self.faces_changed();
    }

    /// Show only triangles of `group`.
    pub fn isolate_group(&mut self, group: u32) {
        let hidden = self.groups.iter().map(|&g| g != group).collect();
        self.set_hidden(hidden);
    }

    /// Hide triangles of `group`, keeping everything else as is.
    pub fn hide_group(&mut self, group: u32) {
        let hidden = self.groups.iter().zip(&self.hidden).map(|(&g, &h)| h || g == group).collect();
        self.set_hidden(hidden);
    }

    pub fn show_all(&mut self) {
        self.set_hidden(vec![false; self.triangles.len()]);
    }

    /// Swap hidden and visible triangles.
    pub fn invert_visibility(&mut self) {
        let hidden = self.hidden.iter().map(|&h| !h).collect();
        self.set_hidden(hidden);
    }

    /// Delete hidden triangles (a topology change).
    pub fn delete_hidden(&self) -> Mesh {
        let mut d = self.to_data();
        let keep: Vec<bool> = d.hidden.iter().map(|&h| !h).collect();
        let mut k = keep.iter();
        d.triangles.retain(|_| *k.next().unwrap_or(&true));
        let mut k = keep.iter();
        d.groups.retain(|_| *k.next().unwrap_or(&true));
        d.hidden.clear();
        Mesh::from_data(d)
    }

    pub fn next_free_group(&self) -> u32 {
        self.groups.iter().max().map_or(0, |m| m + 1)
    }

    /// Assign a new group to visible triangles whose vertices are all masked above 0.5.
    pub fn group_from_mask(&mut self) {
        let g = self.next_free_group();
        let mut groups = self.groups.clone();
        for (t, tri) in self.triangles.iter().enumerate() {
            if !self.hidden[t] && tri.iter().all(|&v| self.mask[v as usize] > 0.5) {
                groups[t] = g;
            }
        }
        self.set_groups(groups);
    }

    /// One group per visible triangle set; hidden triangles keep their group.
    pub fn group_visible(&mut self) {
        let g = self.next_free_group();
        let groups = self.groups.iter().zip(&self.hidden).map(|(&old, &h)| if h { old } else { g }).collect();
        self.set_groups(groups);
    }

    /// One group per connected component.
    pub fn groups_from_islands(&mut self) {
        let mut group = vec![u32::MAX; self.triangles.len()];
        let mut next = 0;
        let mut stack = Vec::new();
        for seed in 0..self.triangles.len() {
            if group[seed] != u32::MAX {
                continue;
            }
            group[seed] = next;
            stack.push(seed as u32);
            while let Some(t) = stack.pop() {
                for &v in &self.triangles[t as usize] {
                    for &n in self.vertex_triangles.get(v as usize) {
                        if group[n as usize] == u32::MAX {
                            group[n as usize] = next;
                            stack.push(n);
                        }
                    }
                }
            }
            next += 1;
        }
        self.set_groups(group);
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
                if self.hidden[ti as usize] {
                    continue;
                }
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
    fn attributes_follow_reordering() {
        let m = primitives::icosphere(3, 1.0);
        let mut d = m.to_data();
        // Group by hemisphere, hide the bottom.
        d.groups = d.triangles.iter().map(|t| u32::from(d.positions[t[0] as usize].y > 0.0)).collect();
        d.hidden = d.groups.iter().map(|&g| g == 0).collect();
        // Shuffle triangle order to force a real reorder.
        let n = d.triangles.len();
        let perm: Vec<usize> = (0..n).map(|i| (i * 7919) % n).collect();
        let d2 = MeshData {
            triangles: perm.iter().map(|&i| d.triangles[i]).collect(),
            groups: perm.iter().map(|&i| d.groups[i]).collect(),
            hidden: perm.iter().map(|&i| d.hidden[i]).collect(),
            ..d.clone()
        };
        let m2 = Mesh::from_data(d2);
        for (t, tri) in m2.triangles.iter().enumerate() {
            let up = m2.positions[tri[0] as usize].y > 0.0;
            assert_eq!(m2.groups()[t], u32::from(up));
            assert_eq!(m2.hidden()[t], !up);
        }
        // Rays hitting only hidden triangles miss.
        assert!(m2.raycast(Vec3::new(0.0, -5.0, 0.0), Vec3::Y).is_some_and(|h| h.point.y > 0.0));
    }

    #[test]
    fn islands_and_isolate() {
        let a = primitives::icosphere(1, 1.0).to_data();
        let mut b = a.clone();
        let off = a.positions.len() as u32;
        b.positions = a.positions.iter().map(|p| *p + Vec3::X * 3.0).collect();
        let mut d = a.clone();
        d.positions.extend(b.positions);
        d.triangles.extend(b.triangles.iter().map(|t| t.map(|i| i + off)));
        let mut m = Mesh::from_data(d);
        m.groups_from_islands();
        assert_eq!(m.next_free_group(), 2);
        let v0 = m.face_version();
        m.isolate_group(m.groups()[0]);
        assert_ne!(v0, m.face_version());
        assert_eq!(m.hidden().iter().filter(|&&h| h).count(), m.triangle_count() / 2);
        let trimmed = m.delete_hidden();
        assert_eq!(trimmed.triangle_count(), m.triangle_count() / 2);
    }

    #[test]
    fn mask_operations() {
        let mut m = primitives::icosphere(3, 1.0);
        // Mask the top cap, then grow/shrink/blur/sharpen it.
        for (i, p) in m.positions.iter().enumerate() {
            m.mask[i] = if p.y > 0.8 { 1.0 } else { 0.0 };
        }
        let count = |m: &Mesh| m.mask.iter().filter(|&&x| x > 0.5).count();
        let base = count(&m);
        m.grow_mask(true);
        assert!(count(&m) > base);
        m.grow_mask(false);
        m.grow_mask(false);
        assert!(count(&m) < base);
        m.blur_mask(2);
        assert!(m.mask.iter().any(|&x| x > 0.0 && x < 1.0), "blur creates soft values");
        m.sharpen_mask();
        m.sharpen_mask();
        m.sharpen_mask();
        assert!(m.mask.iter().filter(|&&x| x > 0.05 && x < 0.95).count() < 10);
    }

    #[test]
    fn curvature_masks() {
        // A sphere with one vertex pushed in: the dent is a cavity, the rest is convex.
        let mut m = primitives::icosphere(3, 1.0);
        let dent = 0;
        m.positions[dent] *= 0.8;
        m.refresh_all();
        m.mask_by_curvature(true);
        assert!(m.mask[dent] > 0.9, "dent should be masked as a cavity: {}", m.mask[dent]);
        let far = (0..m.vertex_count())
            .max_by(|&a, &b| {
                m.positions[a].distance(m.positions[dent]).total_cmp(&m.positions[b].distance(m.positions[dent]))
            })
            .unwrap();
        assert!(m.mask[far] < 0.1);
        m.mask_by_curvature(false);
        assert!(m.mask[dent] < 0.1, "dent is not a peak");
    }

    #[test]
    fn group_mask_and_visibility_growth() {
        let mut m = primitives::icosphere(3, 1.0);
        let groups = m.triangles.iter().map(|t| u32::from(m.positions[t[0] as usize].x > 0.0)).collect();
        m.set_groups(groups);
        m.mask_group(1);
        assert!(m.positions.iter().zip(&m.mask).all(|(p, &k)| p.x <= 0.0 || k == 1.0 || p.x.abs() < 0.2));
        m.isolate_group(1);
        let visible = |m: &Mesh| m.hidden().iter().filter(|&&h| !h).count();
        let v0 = visible(&m);
        m.grow_visible(true);
        assert!(visible(&m) > v0);
        m.grow_visible(false);
        m.grow_visible(false);
        assert!(visible(&m) < v0);
        // Masking skips hidden vertices.
        let hidden_vertex = (0..m.vertex_count() as u32).find(|&v| m.is_vertex_hidden(v)).unwrap();
        let before = m.mask[hidden_vertex as usize];
        m.mask_by_curvature(true);
        assert_eq!(m.mask[hidden_vertex as usize], before);
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
