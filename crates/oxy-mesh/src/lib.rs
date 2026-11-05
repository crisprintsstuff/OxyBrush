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

pub mod primitives;
pub mod topo;

pub use aabb::Aabb;
pub use glam::Vec3;

use adjacency::Csr;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static NEXT_TOPOLOGY_VERSION: AtomicU64 = AtomicU64::new(1);

/// Generation stamps for parallel dedup. Cloning gives a fresh zeroed set:
/// stamps are scratch, not state. (Needed because AtomicU32 isn't Clone and
/// Mesh derives Clone.)
#[derive(Debug, Default)]
struct Stamps(Vec<AtomicU32>);

impl Stamps {
    fn new(n: usize) -> Self {
        Stamps((0..n).map(|_| AtomicU32::new(0)).collect())
    }

    fn reset(&mut self) {
        self.0.iter_mut().for_each(|a| *a.get_mut() = 0);
    }

    /// True for exactly one caller per (id, stamp), even across threads.
    #[inline]
    fn claim(&self, id: u32, stamp: u32) -> bool {
        let a = &self.0[id as usize];
        a.load(Ordering::Relaxed) != stamp && a.swap(stamp, Ordering::Relaxed) != stamp
    }
}

impl Clone for Stamps {
    fn clone(&self) -> Self {
        Stamps::new(self.0.len())
    }
}

/// Target number of triangles per chunk.
pub const CHUNK_TRIANGLES: usize = 1024;

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

/// Color of unpainted vertices.
pub const WHITE: Vec3 = Vec3::ONE;

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
    /// Per vertex color (sRGB, 0..=1); empty means unpainted (white).
    pub color: Vec<Vec3>,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per-vertex mask in `0..=1`; 1 means fully protected from brushes.
    pub mask: Vec<f32>,
    /// Per-vertex color (sRGB, 0..=1). Unpainted vertices are
    /// [`WHITE`], which leaves the viewport material unchanged.
    pub color: Vec<Vec3>,
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
    /// Chunks whose bounds were only grown during a stroke (still correct,
    /// possibly loose); `tighten_bounds` recomputes them exactly.
    loose_bounds: Vec<bool>,
    topology_version: u64,
    vert_stamp: Stamps,
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
        let MeshData { positions, triangles, mask, groups, hidden, color } = data;
        let mask = (mask.len() == positions.len()).then_some(mask);
        // Valid triangles (no repeated or out-of-range corners), keyed by the
        // Morton code of their centroid for a spatial sort. Sorting packed
        // (key, index) u64s in parallel is far cheaper than sorting the full
        // records; the records are gathered in sorted order afterwards.
        let valid = |t: &[u32; 3]| {
            t[0] != t[1] && t[1] != t[2] && t[0] != t[2] && t.iter().all(|&i| (i as usize) < positions.len())
        };
        let bounds = Aabb::from_points(positions.iter().copied());
        let extent = bounds.extent().max(Vec3::splat(1e-12));
        let mut keyed: Vec<u64> = triangles
            .par_iter()
            .enumerate()
            .filter(|(_, t)| valid(t))
            .map(|(i, t)| {
                let c = (positions[t[0] as usize] + positions[t[1] as usize] + positions[t[2] as usize]) / 3.0;
                let n = ((c - bounds.min) / extent).clamp(Vec3::ZERO, Vec3::ONE);
                ((morton3(n) as u64) << 32) | i as u64
            })
            .collect();
        keyed.par_sort_unstable();
        let source_triangle: Vec<u32> = keyed.par_iter().map(|&k| k as u32).collect();
        drop(keyed);
        let mut triangles: Vec<[u32; 3]> = source_triangle.par_iter().map(|&i| triangles[i as usize]).collect();
        let groups: Vec<u32> =
            source_triangle.par_iter().map(|&i| groups.get(i as usize).copied().unwrap_or(0)).collect();
        let hidden: Vec<bool> =
            source_triangle.par_iter().map(|&i| hidden.get(i as usize).copied().unwrap_or(false)).collect();

        // Assign vertex ownership in chunk order and renumber.
        let chunk_count = triangles.len().div_ceil(CHUNK_TRIANGLES).max(1);
        let mut remap = vec![u32::MAX; positions.len()];
        let mut new_positions = Vec::with_capacity(positions.len());
        let mut new_mask = Vec::with_capacity(positions.len());
        let mut new_color = Vec::with_capacity(positions.len());
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
                        new_color.push(color.get(old).copied().unwrap_or(WHITE));
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
            vertex_chunk[c.vertices.start as usize..c.vertices.end as usize].fill(ci as u32);
        }
        let mut triangle_chunk = vec![0u32; triangles.len()];
        for (ci, c) in chunks.iter().enumerate() {
            triangle_chunk[c.triangles.start as usize..c.triangles.end as usize].fill(ci as u32);
        }

        let vertex_triangles = Csr::vertex_triangles(vcount, &triangles);
        let vertex_neighbors = Csr::vertex_neighbors(vcount, &triangles, &vertex_triangles);

        let mut mesh = Mesh {
            normals: vec![Vec3::Z; vcount],
            positions: new_positions,
            mask: new_mask,
            color: new_color,
            groups,
            hidden,
            vertex_hidden: Vec::new(),
            face_version: 0,
            source_vertex,
            source_triangle,
            dirty: vec![true; chunks.len()],
            loose_bounds: vec![false; chunks.len()],
            chunks,
            triangles,
            vertex_triangles,
            vertex_neighbors,
            vertex_chunk,
            triangle_chunk,
            topology_version: NEXT_TOPOLOGY_VERSION.fetch_add(1, Ordering::Relaxed),
            vert_stamp: Stamps::new(vcount),
            stamp: 0,
        };
        mesh.recompute_all_normals();
        let bounds: Vec<Aabb> = (0..mesh.chunks.len()).into_par_iter().map(|c| mesh.chunk_bounds(c)).collect();
        for (chunk, aabb) in mesh.chunks.iter_mut().zip(bounds) {
            chunk.aabb = aabb;
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
            color: if self.is_painted() { self.color.clone() } else { Vec::new() },
        }
    }

    // ---- vertex color ----

    /// Whether any vertex has a color other than white.
    pub fn is_painted(&self) -> bool {
        self.color.iter().any(|&c| c != WHITE)
    }

    /// Blend `color` into every visible vertex, weighted by `strength` and
    /// held back by the mask (a fully masked vertex keeps its color).
    pub fn fill_color(&mut self, color: Vec3, strength: f32) {
        let s = strength.clamp(0.0, 1.0);
        for v in 0..self.color.len() {
            if !self.vertex_hidden[v] {
                let w = s * (1.0 - self.mask[v]);
                self.color[v] = self.color[v].lerp(color, w);
            }
        }
        self.mark_all_dirty();
    }

    /// Like [`fill_color`](Self::fill_color), but only into cavities
    /// (creases, pores, folds) in proportion to how deep they are, or into
    /// peaks (ridges, edges) when `cavities` is false. Good for dirt in the
    /// folds or worn, lighter edges.
    pub fn fill_color_by_curvature(&mut self, color: Vec3, strength: f32, cavities: bool) {
        let s = strength.clamp(0.0, 1.0);
        let curv = self.curvature(cavities);
        for (v, c) in curv.into_iter().enumerate() {
            if !self.vertex_hidden[v] {
                let w = s * c * (1.0 - self.mask[v]);
                self.color[v] = self.color[v].lerp(color, w);
            }
        }
        self.mark_all_dirty();
    }

    /// Reset every visible, unmasked vertex to white.
    pub fn clear_color(&mut self) {
        self.fill_color(WHITE, 1.0);
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
        for (v, c) in self.curvature(cavities).into_iter().enumerate() {
            if !self.vertex_hidden[v] {
                self.mask[v] = c;
            }
        }
    }

    /// Per-vertex concavity (or convexity when `cavities` is false) in
    /// `0..=1`, normalised so the strongest 5% of the mesh reaches 1.
    pub fn curvature(&self, cavities: bool) -> Vec<f32> {
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
        curv.into_iter().map(|c| (c / scale).clamp(0.0, 1.0)).collect()
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

    /// Extract the masked region into a new piece mesh.
    ///
    /// Triangles with average mask >= 0.5 (or >= half of max mask if softly masked) are extracted.
    /// If `thickness > 0.0`, the surface is extruded outward along its normals by `thickness`
    /// and stitched along the boundary with closed side walls to create a watertight solid.
    /// If `thickness <= 0.0`, an open single-sided surface is returned.
    /// `smooth_border` applies Laplacian smoothing iterations to boundary vertices
    /// to eliminate jagged stepping along the rim.
    pub fn extract_mask(&self, thickness: f32, smooth_border: usize) -> Option<MeshData> {
        let max_mask = self.mask.iter().copied().fold(0.0_f32, f32::max);
        if max_mask < 0.05 {
            return None;
        }
        let threshold = 0.5_f32.min(max_mask * 0.7);

        let mut remap = vec![u32::MAX; self.positions.len()];
        let mut front_pos = Vec::new();
        let mut front_normals = Vec::new();
        let mut front_colors = Vec::new();
        let mut front_tris = Vec::new();
        let painted = self.is_painted();

        for (t, &tri) in self.triangles.iter().enumerate() {
            if self.hidden[t] {
                continue;
            }
            let avg_mask = (self.mask[tri[0] as usize] + self.mask[tri[1] as usize] + self.mask[tri[2] as usize]) / 3.0;
            if avg_mask < threshold {
                continue;
            }
            let mapped = tri.map(|v| {
                let r = &mut remap[v as usize];
                if *r == u32::MAX {
                    *r = front_pos.len() as u32;
                    front_pos.push(self.positions[v as usize]);
                    front_normals.push(self.normals[v as usize]);
                    if painted {
                        front_colors.push(self.color[v as usize]);
                    }
                }
                *r
            });
            front_tris.push(mapped);
        }

        if front_tris.is_empty() {
            return None;
        }

        // Identify boundary edges (edges incident to only one triangle in front_tris).
        let mut edge_counts: HashMap<[u32; 2], u32> = HashMap::new();
        for &[a, b, c] in &front_tris {
            for &[u, v] in &[[a, b], [b, c], [c, a]] {
                let key = if u < v { [u, v] } else { [v, u] };
                *edge_counts.entry(key).or_insert(0) += 1;
            }
        }

        let mut boundary_edges = Vec::new();
        for &[a, b, c] in &front_tris {
            for &[u, v] in &[[a, b], [b, c], [c, a]] {
                let key = if u < v { [u, v] } else { [v, u] };
                if edge_counts.get(&key).copied().unwrap_or(0) == 1 {
                    boundary_edges.push([u, v]);
                }
            }
        }

        // Smooth boundary vertices if requested.
        if smooth_border > 0 && !boundary_edges.is_empty() {
            let mut b_adj: HashMap<u32, Vec<u32>> = HashMap::new();
            for &[u, v] in &boundary_edges {
                b_adj.entry(u).or_default().push(v);
                b_adj.entry(v).or_default().push(u);
            }
            for _ in 0..smooth_border {
                let mut smoothed = front_pos.clone();
                for (&v, nbrs) in &b_adj {
                    if !nbrs.is_empty() {
                        let avg: Vec3 =
                            nbrs.iter().map(|&nb| front_pos[nb as usize]).sum::<Vec3>() / (nbrs.len() as f32);
                        smoothed[v as usize] = front_pos[v as usize].lerp(avg, 0.5);
                    }
                }
                front_pos = smoothed;
            }
        }

        if thickness <= 0.0 {
            let n_verts = front_pos.len();
            let n_tris = front_tris.len();
            return Some(MeshData {
                positions: front_pos,
                triangles: front_tris,
                mask: vec![0.0; n_verts],
                color: front_colors,
                groups: vec![0; n_tris],
                hidden: vec![false; n_tris],
            });
        }

        let n = front_pos.len() as u32;
        let mut positions = Vec::with_capacity((n * 2) as usize);
        // Outer / front surface: offset along vertex normals by thickness.
        for (&p, &norm) in front_pos.iter().zip(&front_normals) {
            positions.push(p + norm * thickness);
        }
        // Inner / back surface: original surface positions.
        positions.extend_from_slice(&front_pos);

        let mut triangles = Vec::with_capacity(front_tris.len() * 2 + boundary_edges.len() * 2);
        let mut groups = Vec::with_capacity(triangles.capacity());

        // Front faces (group 0)
        for tri in &front_tris {
            triangles.push(*tri);
            groups.push(0);
        }

        // Back faces with reversed winding (group 1)
        for &[a, b, c] in &front_tris {
            triangles.push([c + n, b + n, a + n]);
            groups.push(1);
        }

        // Side / rim wall faces (group 2)
        for &[u, v] in &boundary_edges {
            // Quad connecting front edge (u -> v) to back edge (v+n -> u+n)
            triangles.push([v, u, u + n]);
            triangles.push([v, u + n, v + n]);
            groups.push(2);
            groups.push(2);
        }

        let color = if painted {
            let mut c = front_colors.clone();
            c.extend_from_slice(&front_colors);
            c
        } else {
            Vec::new()
        };

        let total_verts = positions.len();
        let total_tris = triangles.len();

        Some(MeshData {
            positions,
            triangles,
            mask: vec![0.0; total_verts],
            color,
            groups,
            hidden: vec![false; total_tris],
        })
    }

    /// One group per visible triangle set; hidden triangles keep their group.
    pub fn group_visible(&mut self) {
        let g = self.next_free_group();
        let groups = self.groups.iter().zip(&self.hidden).map(|(&old, &h)| if h { old } else { g }).collect();
        self.set_groups(groups);
    }

    /// One group per connected component.
    pub fn groups_from_islands(&mut self) {
        let group = self.island_ids();
        self.set_groups(group);
    }

    /// Connected-component id of every triangle (triangles sharing a vertex
    /// are connected), numbered from 0 in order of first triangle.
    pub fn island_ids(&self) -> Vec<u32> {
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
        group
    }

    /// Plain data for the triangles where `keep(t)` holds, with unused
    /// vertices dropped (mask, groups and visibility carried along).
    pub fn extract(&self, keep: impl Fn(usize) -> bool) -> MeshData {
        let mut remap = vec![u32::MAX; self.positions.len()];
        let mut out = MeshData::default();
        let painted = self.is_painted();
        for t in (0..self.triangles.len()).filter(|&t| keep(t)) {
            let tri = self.triangles[t].map(|v| {
                let r = &mut remap[v as usize];
                if *r == u32::MAX {
                    *r = out.positions.len() as u32;
                    out.positions.push(self.positions[v as usize]);
                    out.mask.push(self.mask[v as usize]);
                    if painted {
                        out.color.push(self.color[v as usize]);
                    }
                }
                *r
            });
            out.triangles.push(tri);
            out.groups.push(self.groups[t]);
            out.hidden.push(self.hidden[t]);
        }
        out
    }

    /// Split into one mesh per value of `key(t)` (e.g. face group or island),
    /// ordered by key. Returns (key, data) pairs.
    pub fn split_by(&self, key: &[u32]) -> Vec<(u32, MeshData)> {
        let mut keys: Vec<u32> = key.to_vec();
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter().map(|k| (k, self.extract(|t| key[t] == k))).collect()
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

    /// Exact bounds of the mesh (chunks grown during a stroke are measured
    /// precisely here, so the result never includes stale extents).
    pub fn bounds(&self) -> Aabb {
        (0..self.chunks.len()).fold(Aabb::EMPTY, |a, c| {
            a.union(&if self.loose_bounds[c] { self.chunk_bounds(c) } else { self.chunks[c].aabb })
        })
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

    /// Update chunk bounds and mark dirty after an interactive transform.
    pub fn update_chunk_bounds(&mut self, chunk: u32, aabb: Aabb) {
        let c = chunk as usize;
        self.chunks[c].aabb = aabb;
        self.dirty[c] = true;
        self.loose_bounds[c] = true;
    }

    /// Restore exact chunk bounds and mark dirty (e.g. after cancelling a transform).
    pub fn restore_chunk_bounds(&mut self, chunk: u32, aabb: Aabb) {
        let c = chunk as usize;
        self.chunks[c].aabb = aabb;
        self.dirty[c] = true;
        self.loose_bounds[c] = false;
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
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.vert_stamp.reset();
            self.stamp = 1;
        }
        let stamp = self.stamp;
        const PAR_ABOVE: usize = 2048;

        // 1. moved vertices + their neighbours, de-duplicated (parallel when large).
        let verts = collect_unique(moved, |&v| self.vertex_neighbors.get(v as usize), |&v| v, &self.vert_stamp, stamp);

        // 2. normals: unchanged maths, parallel above a threshold so tiny dabs avoid fork/join cost.
        let normals: Vec<Vec3> = if verts.len() > PAR_ABOVE {
            verts.par_iter().map(|&v| self.compute_vertex_normal(v)).collect()
        } else {
            verts.iter().map(|&v| self.compute_vertex_normal(v)).collect()
        };
        for (&v, n) in verts.iter().zip(normals) {
            self.normals[v as usize] = n;
            self.dirty[self.vertex_chunk[v as usize] as usize] = true;
        }

        // 3. bounds: grow each triangle's chunk by the moved corner only, sparse per thread.
        let grow = |part: &[u32]| -> Vec<(u32, Aabb)> {
            let mut acc: Vec<(u32, Aabb)> = Vec::new();
            let mut last = (u32::MAX, 0usize); // cache: neighbours are usually in one chunk
            for &v in part {
                let p = self.positions[v as usize];
                for &t in self.vertex_triangles.get(v as usize) {
                    let c = self.triangle_chunk[t as usize];
                    if last.0 != c {
                        let i = acc.iter().position(|e| e.0 == c).unwrap_or_else(|| {
                            acc.push((c, Aabb::EMPTY));
                            acc.len() - 1
                        });
                        last = (c, i);
                    }
                    acc[last.1].1.grow(p);
                }
            }
            acc
        };
        let boxes: Vec<(u32, Aabb)> = if moved.len() > PAR_ABOVE {
            moved.par_chunks(1024).map(grow).flatten_iter().collect()
        } else {
            grow(moved)
        };
        for (c, b) in boxes {
            let chunk = &mut self.chunks[c as usize];
            chunk.aabb = chunk.aabb.union(&b);
            self.loose_bounds[c as usize] = true;
        }
    }

    /// Recompute exact bounds for chunks that were only grown (call at the
    /// end of a stroke, so later strokes and ray casts test tight boxes).
    pub fn tighten_bounds(&mut self) {
        let loose: Vec<usize> = (0..self.chunks.len()).filter(|&c| self.loose_bounds[c]).collect();
        let boxes: Vec<Aabb> = loose.par_iter().map(|&c| self.chunk_bounds(c)).collect();
        for (c, b) in loose.into_iter().zip(boxes) {
            self.chunks[c].aabb = b;
            self.loose_bounds[c] = false;
        }
    }
}

/// Distinct ids among `items` (via `seed`) and what `expand` yields for them.
fn collect_unique<'a>(
    items: &[u32],
    expand: impl Fn(&u32) -> &'a [u32] + Sync,
    seed: impl Fn(&u32) -> u32 + Sync,
    stamps: &Stamps,
    stamp: u32,
) -> Vec<u32> {
    const SERIAL_BELOW: usize = 4096;
    let run = |part: &[u32]| -> Vec<u32> {
        let mut out = Vec::with_capacity(part.len() * 2);
        for item in part {
            let s = seed(item);
            if stamps.claim(s, stamp) {
                out.push(s);
            }
            for &id in expand(item) {
                if stamps.claim(id, stamp) {
                    out.push(id);
                }
            }
        }
        out
    };
    if items.len() <= SERIAL_BELOW {
        return run(items);
    }
    let parts: Vec<Vec<u32>> = items.par_chunks(1024).map(run).collect();
    let mut out = Vec::with_capacity(parts.iter().map(Vec::len).sum());
    parts.into_iter().for_each(|mut p| out.append(&mut p));
    out
}

impl Mesh {
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

    /// Distance-weighted average of the vertex normals within `radius` of
    /// `point`, walked outward from `triangle`.
    ///
    /// A single triangle's geometric normal jumps from facet to facet on a
    /// dense mesh. Averaging the vertices under the brush, with a smooth
    /// falloff that is 1 at `point` and 0 at `radius`, stays put when the
    /// pointer steps onto the next tiny triangle and when a vertex crosses
    /// the rim of the disc.
    ///
    /// `toward` (the direction toward the camera) drops vertices that face
    /// the other way, so the two sides of a thin part are not blended into
    /// nothing. The walk stops after a few hundred vertices: further samples
    /// are the same neighborhood and only cost time on a dense mesh. This
    /// is a cursor query, not a brush dab.
    ///
    /// PRECONDITION: `triangle` indexes a triangle of this mesh. A radius of
    /// zero or less returns the normal at the hit point.
    ///
    /// RETURNS: a unit normal, facing `toward` when `toward` is not zero.
    /// When the neighborhood is empty or cancels out, the barycentric blend
    /// of the hit triangle's vertex normals, then that triangle's geometric
    /// normal.
    pub fn averaged_normal(&self, triangle: u32, point: Vec3, radius: f32, toward: Vec3) -> Vec3 {
        let toward = toward.normalize_or_zero();
        if triangle as usize >= self.triangles.len() {
            return orient_normal(Vec3::Y, toward);
        }
        let geometric = self.face_normal_unnormalized(triangle).normalize_or(Vec3::Y);
        let interpolated = self.interpolated_normal(triangle, point);
        let fallback = if interpolated.length_squared() > 0.5 { interpolated } else { geometric };
        if radius <= 1e-8 {
            return orient_normal(fallback, toward);
        }

        // The first rings around the hit decide the average. Capping the walk
        // keeps a large brush on a dense mesh to a few thousand neighbor lookups.
        const MAX_VERTICES: usize = 512;
        let radius_sq = radius * radius;
        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        for v in self.triangles[triangle as usize] {
            if seen.insert(v) {
                queue.push_back(v);
            }
        }

        let mut sum = fallback;
        let mut weight = 1.0;
        let mut counted = 0usize;
        while let Some(v) = queue.pop_front() {
            let vi = v as usize;
            if vi >= self.positions.len() || self.vertex_hidden[vi] {
                continue;
            }
            let dist_sq = self.positions[vi].distance_squared(point);
            if dist_sq > radius_sq {
                continue;
            }
            if counted >= MAX_VERTICES {
                break;
            }
            counted += 1;
            let n = self.normals[vi];
            if toward == Vec3::ZERO || n.dot(toward) > 0.0 {
                let t = (dist_sq.sqrt() / radius).clamp(0.0, 1.0);
                let u = 1.0 - t;
                // Smoothstep: zero slope at the hit and at the rim, so a vertex
                // sliding into the disc does not tug the average.
                let w = u * u * (3.0 - 2.0 * u);
                sum += n * w;
                weight += w;
            }
            for &nb in self.neighbors(v) {
                if seen.insert(nb) {
                    queue.push_back(nb);
                }
            }
        }
        orient_normal((sum / weight).normalize_or(geometric), toward)
    }

    /// Barycentric blend of the triangle's vertex normals at `point`.
    fn interpolated_normal(&self, triangle: u32, point: Vec3) -> Vec3 {
        let [ia, ib, ic] = self.triangles[triangle as usize];
        let (a, b, c) = (self.positions[ia as usize], self.positions[ib as usize], self.positions[ic as usize]);
        let v0 = b - a;
        let v1 = c - a;
        let v2 = point - a;
        let d00 = v0.dot(v0);
        let d01 = v0.dot(v1);
        let d11 = v1.dot(v1);
        let denom = d00 * d11 - d01 * d01;
        if denom.abs() < 1e-20 {
            return self.normals[ia as usize].normalize_or_zero();
        }
        let d20 = v2.dot(v0);
        let d21 = v2.dot(v1);
        let bv = (d11 * d20 - d01 * d21) / denom;
        let bw = (d00 * d21 - d01 * d20) / denom;
        let bu = 1.0 - bv - bw;
        (self.normals[ia as usize] * bu + self.normals[ib as usize] * bv + self.normals[ic as usize] * bw)
            .normalize_or_zero()
    }
}

/// Flip `normal` so it points with `toward`. A zero `toward` leaves it alone.
fn orient_normal(normal: Vec3, toward: Vec3) -> Vec3 {
    if toward != Vec3::ZERO && normal.dot(toward) < 0.0 { -normal } else { normal }
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
    fn vertex_color_survives_reorder_subdivision_and_levels() {
        let mut m = primitives::icosphere(2, 1.0);
        assert!(!m.is_painted() && m.to_data().color.is_empty(), "unpainted meshes carry no color data");
        for v in 0..m.vertex_count() {
            m.color[v] = if m.positions[v].x > 0.0 { Vec3::new(1.0, 0.0, 0.0) } else { WHITE };
        }
        // Rebuilding from data keeps each vertex's color with its position.
        let r = Mesh::from_data(m.to_data());
        for v in 0..r.vertex_count() {
            assert_eq!(r.color[v].y == 0.0, r.positions[v].x > 0.0);
        }
        // Subdivision: midpoints average their edge.
        let s = topo::subdivide(&m, 0);
        assert_eq!(s.color.len(), s.vertex_count());
        assert!(s.color.iter().any(|c| (c.y - 0.5).abs() < 1e-6), "edge midpoints across the boundary blend");
        // Subdivision levels carry color up and back down.
        let mut mr = multires::Multires::new(&m);
        let up = Mesh::from_data(mr.add_level(&m));
        assert!(up.is_painted() && up.color.len() == up.vertex_count());
        mr.bind(&up);
        let down = Mesh::from_data(mr.set_level(&up, 0));
        assert_eq!(down.color.len(), m.vertex_count());
        assert_eq!(down.color.iter().filter(|c| c.y == 0.0).count(), m.color.iter().filter(|c| c.y == 0.0).count());
    }

    #[test]
    fn color_fills_follow_curvature_and_mask() {
        let mut m = primitives::icosphere(4, 1.0);
        // A dent: push one vertex in.
        let dent = 0usize;
        m.positions[dent] *= 0.8;
        m.recompute_all_normals();
        let black = Vec3::ZERO;
        m.fill_color_by_curvature(black, 1.0, true);
        assert!(m.color[dent].x < 0.2, "the dent gets dirt: {}", m.color[dent]);
        let far = (0..m.vertex_count()).max_by(|&a, &b| {
            m.positions[a].distance(m.positions[dent]).total_cmp(&m.positions[b].distance(m.positions[dent]))
        });
        assert!(m.color[far.unwrap()].x > 0.8, "smooth areas stay clean");
        m.mask.iter_mut().for_each(|x| *x = 1.0);
        m.clear_color();
        assert!(m.color[dent].x < 0.2, "masked vertices keep their color");
        m.mask.iter_mut().for_each(|x| *x = 0.0);
        m.clear_color();
        assert!(!m.is_painted());
    }

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
    fn averaged_normal_follows_a_sphere_and_faces_the_camera() {
        let m = primitives::icosphere(4, 1.0);
        let hit = m.raycast(Vec3::new(0.2, 0.1, 4.0), -Vec3::Z).unwrap();
        let n = m.averaged_normal(hit.triangle, hit.point, 0.3, Vec3::Z);
        assert!((n.length() - 1.0).abs() < 1e-4, "{n}");
        assert!(n.dot(hit.point.normalize()) > 0.98, "outward: {n} vs {}", hit.point.normalize());
        // Samples that face away are dropped; the result still faces `toward`.
        let away = m.averaged_normal(hit.triangle, hit.point, 0.3, -Vec3::Z);
        assert!(away.dot(-Vec3::Z) > 0.5, "{away}");
        let at_point = m.averaged_normal(hit.triangle, hit.point, 0.0, Vec3::Z);
        assert!((at_point.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn averaged_normal_stays_steady_across_bumps() {
        let mut data = primitives::plane(48, 2.0).to_data();
        for p in &mut data.positions {
            p.y += (p.x * 80.0).sin() * (p.z * 80.0).cos() * 0.05;
        }
        let m = Mesh::from_data(data);
        let h1 = m.raycast(Vec3::new(0.04, 3.0, 0.02), -Vec3::Y).unwrap();
        let h2 = m.raycast(Vec3::new(0.09, 3.0, 0.07), -Vec3::Y).unwrap();
        let a1 = m.averaged_normal(h1.triangle, h1.point, 0.35, Vec3::Y);
        let a2 = m.averaged_normal(h2.triangle, h2.point, 0.35, Vec3::Y);
        assert!(
            h1.normal.dot(h2.normal) < 0.995,
            "neighboring facets should actually tilt: {}",
            h1.normal.dot(h2.normal)
        );
        assert!(a1.dot(a2) > 0.999, "averaged normals twitched: {}", a1.dot(a2));
        assert!(a1.dot(Vec3::Y) > 0.95, "bumps leaked into the average: {a1}");
        assert!(a2.dot(Vec3::Y) > 0.95, "{a2}");
    }

    #[test]
    fn extract_mask_solid_and_flat() {
        let mut m = primitives::icosphere(3, 1.0);
        assert!(m.extract_mask(0.02, 1).is_none(), "no mask: returns None");

        // Mask the top hemisphere
        for (i, p) in m.positions.iter().enumerate() {
            if p.y > 0.3 {
                m.mask[i] = 1.0;
            }
        }

        // 1. Flat extraction (thickness = 0)
        let flat = m.extract_mask(0.0, 0).expect("should extract flat patch");
        assert!(!flat.positions.is_empty());
        assert!(!flat.triangles.is_empty());
        assert!(flat.positions.iter().all(|p| p.y > 0.2));
        assert!(flat.mask.iter().all(|&k| k == 0.0), "extracted mask is cleared for sculpting");

        // 2. Solid extraction with thickness
        let solid_data = m.extract_mask(0.05, 2).expect("should extract solid shell");
        let solid = Mesh::from_data(solid_data);
        assert!(solid.positions.len() > flat.positions.len());
        // Verify groups: 0 for front, 1 for back, 2 for rim
        assert!(solid.groups.contains(&0));
        assert!(solid.groups.contains(&1));
        assert!(solid.groups.contains(&2));
        // Verify the front vertices were extruded outward along normals
        let max_y = solid.positions.iter().map(|p| p.y).fold(0.0_f32, f32::max);
        assert!(max_y > 1.04, "top vertex should be extruded outward: {max_y}");
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
    fn extract_and_split_keep_attributes() {
        // Two separate spheres in one mesh.
        let a = primitives::icosphere(2, 1.0).to_data();
        let mut data = a.clone();
        let n = a.positions.len() as u32;
        data.positions.extend(a.positions.iter().map(|p| *p + Vec3::X * 5.0));
        data.triangles.extend(a.triangles.iter().map(|t| t.map(|v| v + n)));
        data.mask = vec![0.0; data.positions.len()];
        data.mask[0] = 1.0;
        // Paint the second sphere red.
        data.color = (0..data.positions.len()).map(|v| if v as u32 >= n { Vec3::X } else { WHITE }).collect();
        let m = Mesh::from_data(data);
        let islands = m.island_ids();
        let parts = m.split_by(&islands);
        assert_eq!(parts.len(), 2);
        for (_, d) in &parts {
            assert_eq!(d.triangles.len(), a.triangles.len());
            assert_eq!(d.positions.len(), a.positions.len(), "unused vertices dropped");
            assert!(d.triangles.iter().flatten().all(|&v| (v as usize) < d.positions.len()));
        }
        let masked: f32 = parts.iter().map(|(_, d)| d.mask.iter().sum::<f32>()).sum();
        assert_eq!(masked, 1.0, "mask carried to the right part");
        let red: Vec<bool> = parts.iter().map(|(_, d)| d.color.iter().all(|&c| c == Vec3::X)).collect();
        assert_eq!(red.iter().filter(|&&r| r).count(), 1, "paint carried to the right part");
        assert!(parts.iter().all(|(_, d)| d.color.len() == d.positions.len()));
        assert!(m.extract(|_| false).triangles.is_empty());
    }

    #[test]
    fn bounds_grow_during_strokes_and_tighten_after() {
        let mut m = primitives::icosphere(5, 1.0);
        let exact: Vec<Aabb> = m.chunks.iter().map(|c| c.aabb).collect();
        // Pull a patch far out, then back: grown bounds stay large but valid.
        let patch: Vec<u32> = (0..m.vertex_count() as u32).filter(|&v| m.positions[v as usize].z > 0.95).collect();
        for &v in &patch {
            m.positions[v as usize] *= 2.0;
        }
        m.update_after_move(&patch);
        for &v in &patch {
            m.positions[v as usize] *= 0.5;
        }
        m.update_after_move(&patch);
        for (ci, c) in m.chunks.iter().enumerate() {
            for t in c.triangles.clone() {
                for v in m.triangles[t as usize] {
                    assert!(c.aabb.contains(m.positions[v as usize]), "chunk {ci} must contain its corners");
                }
            }
        }
        let grown = m.chunks.iter().zip(&exact).any(|(c, e)| c.aabb.max.z > e.max.z + 0.5);
        assert!(grown, "bounds were grown, not refit, during the stroke");
        m.tighten_bounds();
        for (c, e) in m.chunks.iter().zip(&exact) {
            assert!((c.aabb.min - e.min).length() < 1e-5 && (c.aabb.max - e.max).length() < 1e-5);
        }
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

    #[test]
    fn update_after_move_parallel_and_serial_equivalence() {
        let mut m = primitives::icosphere(6, 1.0);
        let vcount = m.vertex_count() as u32;

        for round in 0..6 {
            let count = match round % 3 {
                0 => 300,
                1 => 3_000,
                _ => 20_000,
            };
            let step = (vcount / count).max(1);
            let moved: Vec<u32> = (0..vcount).step_by(step as usize).take(count as usize).collect();

            for &v in &moved {
                m.positions[v as usize] += Vec3::new(0.01, 0.02, -0.01) * ((round + 1) as f32);
            }
            m.update_after_move(&moved);

            let mut ref_mesh = m.clone();
            ref_mesh.recompute_all_normals();
            for v in 0..vcount {
                let diff = (m.normals[v as usize] - ref_mesh.normals[v as usize]).length();
                assert!(diff < 1e-4, "normal mismatch at vertex {v} in round {round}: diff {diff}");
            }

            for (ci, c) in m.chunks.iter().enumerate() {
                for t in c.triangles.clone() {
                    for v in m.triangles[t as usize] {
                        assert!(c.aabb.contains(m.positions[v as usize]), "chunk {ci} must contain vertex {v}");
                    }
                }
            }
        }
    }

    #[test]
    fn cloned_mesh_updates_independently() {
        let mut m1 = primitives::icosphere(4, 1.0);
        let mut m2 = m1.clone();

        m1.positions[0] *= 2.0;
        m1.update_after_move(&[0]);

        assert_ne!(m1.normals[0], m2.normals[0]);
        m2.positions[0] *= 3.0;
        m2.update_after_move(&[0]);

        assert_ne!(m1.positions[0], m2.positions[0]);
        assert_ne!(m1.normals[0], m2.normals[0]);
    }
}
