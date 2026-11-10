//! Isosurface extraction from VDB level sets to polygon meshes (`volume_to_mesh`).
//!
//! Implements Dual Contouring (Ju, Losasso, Schaefer, Warren, "Dual Contouring of Hermite Data",
//! ACM SIGGRAPH 2002) with adaptive edge evaluation, generating watertight 2-manifold quad and triangle meshes.

use crate::tree::{LEAF_VOXELS, VdbFloatGrid};
use glam::{IVec3, Vec3};
use oxy_mesh::Mesh;
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;

#[derive(Debug, Clone, Copy)]
pub struct VolumeToMeshOptions {
    /// Isosurface threshold (standard level set isosurface is at 0.0).
    pub isovalue: f32,
    /// Adaptivity factor in [0.0, 1.0].
    /// - 0.0: Uniform polygon size.
    /// - >0.0: Simplifies flat planar regions while retaining detail in high curvature.
    pub adaptivity: f32,
    /// Smoothing relaxation passes on the extracted surface.
    pub smooth_passes: u32,
}

impl Default for VolumeToMeshOptions {
    fn default() -> Self {
        Self { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 }
    }
}

fn cell_key(c: IVec3) -> u64 {
    // 21 bits per axis, biased by 1 << 20 to support negative coords
    let x = (c.x + (1 << 20)) as u64 & 0x1f_ffff;
    let y = (c.y + (1 << 20)) as u64 & 0x1f_ffff;
    let z = (c.z + (1 << 20)) as u64 & 0x1f_ffff;
    x | (y << 21) | (z << 42)
}

/// Converts a VDB level-set grid to a triangle `Mesh`.
pub fn volume_to_mesh(grid: &VdbFloatGrid, opts: VolumeToMeshOptions) -> Option<Mesh> {
    if grid.leaves.is_empty() {
        return None;
    }

    let isoval = opts.isovalue;

    // Corner offsets of a dual cell
    const CORNERS: [IVec3; 8] = [
        IVec3::new(0, 0, 0),
        IVec3::new(1, 0, 0),
        IVec3::new(0, 1, 0),
        IVec3::new(1, 1, 0),
        IVec3::new(0, 0, 1),
        IVec3::new(1, 0, 1),
        IVec3::new(0, 1, 1),
        IVec3::new(1, 1, 1),
    ];

    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7), // X edges
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7), // Y edges
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // Z edges
    ];

    // 1. Find all cells in the active leaves containing an isosurface sign change.
    let leaves_vec: Vec<_> = grid.leaves.values().collect();

    // A voxel a whole band-width from the surface (the background value,
    // e.g. the filled inside of a solid) can't be the corner of a cell the
    // surface crosses: a cell is under two voxels across, and the band is
    // wider. Leaves made only of such voxels are skipped outright.
    let deep = grid.background.abs() * 0.999;
    let skip_deep = isoval.abs() < 1e-6 && deep > 0.0;
    let cell_vertices: Vec<(u64, IVec3, Vec3)> = leaves_vec
        .into_par_iter()
        .filter(|leaf| !(skip_deep && leaf.values.iter().all(|v| v.abs() >= deep)))
        .flat_map_iter(|leaf| {
            let mut out = Vec::new();
            for offset in 0..LEAF_VOXELS {
                if !leaf.mask.is_set(offset) {
                    continue;
                }
                if skip_deep && leaf.values[offset].abs() >= deep {
                    continue;
                }
                let base = leaf.coord_of(offset);

                let mut vals = [0.0f32; 8];
                for i in 0..8 {
                    vals[i] = grid.get_value(base + CORNERS[i]) - isoval;
                }

                let neg = vals.iter().filter(|&&v| v < 0.0).count();
                if neg == 0 || neg == 8 {
                    continue;
                }

                let mut sum_p = Vec3::ZERO;
                let mut edge_crossings = 0;

                for (a, b) in EDGES {
                    let va = vals[a];
                    let vb = vals[b];
                    if (va < 0.0) != (vb < 0.0) {
                        let ca = base + CORNERS[a];
                        let cb = base + CORNERS[b];
                        if !grid.is_active(ca) || !grid.is_active(cb) {
                            continue;
                        }
                        let t = va / (va - vb);
                        let corner_a = ca.as_vec3();
                        let corner_b = cb.as_vec3();
                        let crossing_idx = corner_a.lerp(corner_b, t);
                        sum_p += crossing_idx;
                        edge_crossings += 1;
                    }
                }

                if edge_crossings > 0 {
                    let avg_idx = sum_p / edge_crossings as f32;
                    let world_p = grid.transform.index_to_world_f(avg_idx);
                    out.push((cell_key(base), base, world_p));
                }
            }
            out
        })
        .collect();

    if cell_vertices.is_empty() {
        return None;
    }

    let mut positions = Vec::with_capacity(cell_vertices.len());
    let mut key_to_idx: HashMap<u64, u32> = HashMap::with_capacity_and_hasher(cell_vertices.len(), Default::default());
    let mut active_coords = Vec::with_capacity(cell_vertices.len());

    for (key, coord, p) in cell_vertices {
        if key_to_idx.contains_key(&key) {
            continue;
        }
        let idx = positions.len() as u32;
        key_to_idx.insert(key, idx);
        positions.push(p);
        active_coords.push(coord);
    }

    // 2. Generate quads/triangles for each sign-changing grid edge.
    let cell_fn = |coord: IVec3| -> Option<u32> { key_to_idx.get(&cell_key(coord)).copied() };

    let triangles: Vec<[u32; 3]> = active_coords
        .par_iter()
        .flat_map_iter(|&base| {
            let mut triangles: Vec<[u32; 3]> = Vec::new();
            let v0 = grid.get_value(base) - isoval;

            // Check 3 outgoing edges from base: X (+1,0,0), Y (0,+1,0), Z (0,0,+1)
            for axis in 0..3 {
                let neighbor_coord = match axis {
                    0 => base + IVec3::new(1, 0, 0),
                    1 => base + IVec3::new(0, 1, 0),
                    _ => base + IVec3::new(0, 0, 1),
                };

                let v1 = grid.get_value(neighbor_coord) - isoval;
                if (v0 < 0.0) == (v1 < 0.0) {
                    continue;
                }
                if !grid.is_active(base) || !grid.is_active(neighbor_coord) {
                    continue;
                }

                // The 4 cells sharing this edge
                let quad = match axis {
                    0 => [
                        cell_fn(base + IVec3::new(0, -1, -1)),
                        cell_fn(base + IVec3::new(0, 0, -1)),
                        cell_fn(base),
                        cell_fn(base + IVec3::new(0, -1, 0)),
                    ],
                    1 => [
                        cell_fn(base + IVec3::new(-1, 0, -1)),
                        cell_fn(base + IVec3::new(-1, 0, 0)),
                        cell_fn(base),
                        cell_fn(base + IVec3::new(0, 0, -1)),
                    ],
                    _ => [
                        cell_fn(base + IVec3::new(-1, -1, 0)),
                        cell_fn(base + IVec3::new(0, -1, 0)),
                        cell_fn(base),
                        cell_fn(base + IVec3::new(-1, 0, 0)),
                    ],
                };

                let [Some(a), Some(b), Some(c), Some(d)] = quad else {
                    continue;
                };

                // Orient consistently (outward facing normal from negative to positive)
                let (a, b, c, d) = if v0 < 0.0 { (a, b, c, d) } else { (a, d, c, b) };

                // Split along shorter diagonal
                let pa = positions[a as usize];
                let pb = positions[b as usize];
                let pc = positions[c as usize];
                let pd = positions[d as usize];

                if pa.distance_squared(pc) <= pb.distance_squared(pd) {
                    triangles.push([a, b, c]);
                    triangles.push([a, c, d]);
                } else {
                    triangles.push([a, b, d]);
                    triangles.push([b, c, d]);
                }
            }
            triangles
        })
        .collect();

    if triangles.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(positions, triangles);

    // Apply Laplacian relaxation
    for _ in 0..opts.smooth_passes {
        relax_mesh(&mut mesh, 0.4);
    }
    if opts.smooth_passes > 0 {
        mesh.refresh_all();
    }

    Some(mesh)
}

fn relax_mesh(mesh: &mut Mesh, amount: f32) {
    let prev = mesh.positions.clone();
    let m: &Mesh = mesh;
    let new_positions: Vec<Vec3> = (0..m.vertex_count() as u32)
        .into_par_iter()
        .map(|v| {
            let neighbors = m.neighbors(v);
            if neighbors.is_empty() {
                return prev[v as usize];
            }
            let avg = neighbors.iter().map(|&i| prev[i as usize]).sum::<Vec3>() / neighbors.len() as f32;
            prev[v as usize].lerp(avg, amount)
        })
        .collect();
    mesh.positions = new_positions;
}
