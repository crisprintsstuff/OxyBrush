//! Conversion of polygon meshes into narrow-band VDB level-set grids (`mesh_to_level_set`).
//!
//! Signed distance computation uses angle-weighted vertex pseudonormals
//! (Bærentzen & Aanæs, "Signed Distance Computation Using the Angle Weighted Pseudonormal", IEEE TVCG 2005)
//! and closest-point triangle projections (Ericson, "Real-Time Collision Detection", 2005) followed by
//! flood-fill exterior signing.

use crate::tree::{LEAF_DIM, LEAF_VOXELS, LeafNode, Transform, VdbFloatGrid};
use glam::{IVec3, Vec3};
use rayon::prelude::*;
use std::collections::HashMap;

/// Closest point on triangle (Ericson, Real-Time Collision Detection 5.1.5),
/// returning both the closest point and the barycentric coordinates (u, v, w) on the triangle.
pub fn closest_point_and_barycentric(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> (Vec3, Vec3) {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;

    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, Vec3::new(1.0, 0.0, 0.0));
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, Vec3::new(0.0, 1.0, 0.0));
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, Vec3::new(1.0 - v, v, 0.0));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, Vec3::new(0.0, 0.0, 1.0));
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (a + ac * w, Vec3::new(1.0 - w, 0.0, w));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * w, Vec3::new(0.0, 1.0 - w, w));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    let u = 1.0 - v - w;
    (a + ab * v + ac * w, Vec3::new(u, v, w))
}

pub fn closest_point_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    closest_point_and_barycentric(p, a, b, c).0
}

#[derive(Debug, Clone, Copy)]
pub struct MeshToVolumeOptions {
    /// Voxel dimensions in world space.
    pub voxel_size: f32,
    /// Half-width of the narrow band in voxel units (e.g. 3.0).
    pub half_width: f32,
}

impl Default for MeshToVolumeOptions {
    fn default() -> Self {
        Self { voxel_size: 0.05, half_width: 3.0 }
    }
}

/// Converts a triangle mesh into a sparse narrow-band VDB level set (`FloatGrid`).
pub fn mesh_to_level_set(positions: &[Vec3], triangles: &[[u32; 3]], opts: MeshToVolumeOptions) -> VdbFloatGrid {
    let voxel_size = opts.voxel_size.max(1e-5);
    let half_width = opts.half_width.max(1.0);
    let band_dist = half_width * voxel_size;
    let bg = band_dist;

    let mut grid = VdbFloatGrid::new("surface", voxel_size, bg);
    if positions.is_empty() || triangles.is_empty() {
        return grid;
    }

    let transform = Transform::new(voxel_size, Vec3::ZERO);
    grid.transform = transform;

    // 1. Identify which 8x8x8 leaf nodes intersect the narrow band of any triangle.
    let leaf_tris: HashMap<IVec3, Vec<usize>> = {
        let tri_leaf_pairs: Vec<(IVec3, usize)> = triangles
            .par_iter()
            .enumerate()
            .flat_map(|(ti, tri)| {
                let p0 = positions[tri[0] as usize];
                let p1 = positions[tri[1] as usize];
                let p2 = positions[tri[2] as usize];

                let min_p = p0.min(p1).min(p2) - Vec3::splat(band_dist);
                let max_p = p0.max(p1).max(p2) + Vec3::splat(band_dist);

                let min_idx = transform.world_to_index(min_p);
                let max_idx = transform.world_to_index(max_p);

                let min_leaf = VdbFloatGrid::leaf_origin(min_idx);
                let max_leaf = VdbFloatGrid::leaf_origin(max_idx);

                let mut leaves = Vec::new();
                let mut lz = min_leaf.z;
                while lz <= max_leaf.z {
                    let mut ly = min_leaf.y;
                    while ly <= max_leaf.y {
                        let mut lx = min_leaf.x;
                        while lx <= max_leaf.x {
                            leaves.push((IVec3::new(lx, ly, lz), ti));
                            lx += LEAF_DIM;
                        }
                        ly += LEAF_DIM;
                    }
                    lz += LEAF_DIM;
                }
                leaves
            })
            .collect();

        let mut map: HashMap<IVec3, Vec<usize>> = HashMap::new();
        for (leaf_orig, ti) in tri_leaf_pairs {
            map.entry(leaf_orig).or_default().push(ti);
        }
        map
    };

    if leaf_tris.is_empty() {
        return grid;
    }

    // Precompute angle-weighted vertex pseudonormals for exact watertight inside/outside classification.
    let vert_normals: Vec<Vec3> = {
        let mut v_norms = vec![Vec3::ZERO; positions.len()];
        for tri in triangles {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            let p0 = positions[i0];
            let p1 = positions[i1];
            let p2 = positions[i2];
            let e0 = (p1 - p0).normalize_or_zero();
            let e1 = (p2 - p1).normalize_or_zero();
            let e2 = (p0 - p2).normalize_or_zero();
            let a0 = (-e2).dot(e0).clamp(-1.0, 1.0).acos();
            let a1 = (-e0).dot(e1).clamp(-1.0, 1.0).acos();
            let a2 = (-e1).dot(e2).clamp(-1.0, 1.0).acos();
            let fnorm = (p1 - p0).cross(p2 - p0).normalize_or_zero();
            v_norms[i0] += fnorm * a0;
            v_norms[i1] += fnorm * a1;
            v_norms[i2] += fnorm * a2;
        }
        for n in &mut v_norms {
            *n = n.normalize_or_zero();
        }
        v_norms
    };

    // 2. Compute narrow-band signed distance values per leaf node in parallel.
    struct LeafResult {
        origin: IVec3,
        distances: [f32; LEAF_VOXELS],
        normals: [Vec3; LEAF_VOXELS],
        closest_pts: [Vec3; LEAF_VOXELS],
        has_active: bool,
    }

    let leaves_vec: Vec<(IVec3, Vec<usize>)> = leaf_tris.into_iter().collect();

    let processed_leaves: Vec<LeafResult> = leaves_vec
        .into_par_iter()
        .map(|(origin, mut tis)| {
            tis.sort_unstable();
            tis.dedup();

            let mut distances = [bg; LEAF_VOXELS];
            let mut normals = [Vec3::ZERO; LEAF_VOXELS];
            let mut closest_pts = [Vec3::ZERO; LEAF_VOXELS];
            let mut has_active = false;

            for offset in 0..LEAF_VOXELS {
                let x = (offset & 7) as i32;
                let y = ((offset >> 3) & 7) as i32;
                let z = ((offset >> 6) & 7) as i32;
                let coord = origin + IVec3::new(x, y, z);
                let p = transform.index_to_world(coord);

                let mut best_dist = f32::MAX;
                let mut best_normal = Vec3::ZERO;
                let mut best_c = p;

                for &ti in &tis {
                    let tri = &triangles[ti];
                    let i0 = tri[0] as usize;
                    let i1 = tri[1] as usize;
                    let i2 = tri[2] as usize;
                    let v0 = positions[i0];
                    let v1 = positions[i1];
                    let v2 = positions[i2];

                    let (c, bary) = closest_point_and_barycentric(p, v0, v1, v2);
                    let d = p.distance(c);
                    if d < best_dist {
                        best_dist = d;
                        best_c = c;
                        let n = bary.x * vert_normals[i0] + bary.y * vert_normals[i1] + bary.z * vert_normals[i2];
                        let len = n.length();
                        best_normal = if len > 1e-6 { n / len } else { Vec3::Y };
                    }
                }

                distances[offset] = best_dist.min(bg);
                normals[offset] = best_normal;
                closest_pts[offset] = best_c;
                if best_dist < bg {
                    has_active = true;
                }
            }

            LeafResult { origin, distances, normals, closest_pts, has_active }
        })
        .filter(|res| res.has_active)
        .collect();

    // 3. Assemble leaves and assign signs.
    for res in processed_leaves {
        let mut leaf = LeafNode::new(res.origin, bg);
        for offset in 0..LEAF_VOXELS {
            let d = res.distances[offset];
            let coord = leaf.coord_of(offset);
            let p = transform.index_to_world(coord);
            let c = res.closest_pts[offset];
            let n = res.normals[offset];

            let diff = p - c;
            let outside = if diff.length_squared() > 1e-10 { diff.dot(n) >= 0.0 } else { true };

            let signed_d = if outside { d } else { -d };
            if d < bg || !outside {
                leaf.set(offset, signed_d.clamp(-bg, bg));
            } else {
                leaf.values[offset] = signed_d;
            }
        }
        grid.leaves.insert(res.origin, leaf);
    }

    // 4. Identify and fill enclosed interior leaves for solid closed meshes.
    let mut min_l = IVec3::splat(i32::MAX);
    let mut max_l = IVec3::splat(i32::MIN);
    for &orig in grid.leaves.keys() {
        min_l = min_l.min(orig);
        max_l = max_l.max(orig);
    }

    if min_l.x <= max_l.x {
        let pad = LEAF_DIM;
        let box_min = min_l - IVec3::splat(pad);
        let box_max = max_l + IVec3::splat(pad);

        let mut leaf_set: std::collections::HashSet<IVec3> = std::collections::HashSet::new();
        let mut lz = box_min.z;
        while lz <= box_max.z {
            let mut ly = box_min.y;
            while ly <= box_max.y {
                let mut lx = box_min.x;
                while lx <= box_max.x {
                    leaf_set.insert(IVec3::new(lx, ly, lz));
                    lx += LEAF_DIM;
                }
                ly += LEAF_DIM;
            }
            lz += LEAF_DIM;
        }

        // Leaves that intersect the surface boundary
        let blocking: std::collections::HashSet<IVec3> = grid.leaves.keys().copied().collect();

        let mut outside_leaves: std::collections::HashSet<IVec3> = std::collections::HashSet::new();
        let mut queue: Vec<IVec3> = Vec::new();

        for &l in &leaf_set {
            let is_border = l.x == box_min.x
                || l.x == box_max.x
                || l.y == box_min.y
                || l.y == box_max.y
                || l.z == box_min.z
                || l.z == box_max.z;
            if is_border && !blocking.contains(&l) {
                outside_leaves.insert(l);
                queue.push(l);
            }
        }

        const NEIGHBORS: [IVec3; 6] = [
            IVec3::new(LEAF_DIM, 0, 0),
            IVec3::new(-LEAF_DIM, 0, 0),
            IVec3::new(0, LEAF_DIM, 0),
            IVec3::new(0, -LEAF_DIM, 0),
            IVec3::new(0, 0, LEAF_DIM),
            IVec3::new(0, 0, -LEAF_DIM),
        ];

        while let Some(curr) = queue.pop() {
            for &n in &NEIGHBORS {
                let neighbor = curr + n;
                if leaf_set.contains(&neighbor) && !blocking.contains(&neighbor) && outside_leaves.insert(neighbor) {
                    queue.push(neighbor);
                }
            }
        }

        let mut interior_leaves = Vec::new();
        for &l in &leaf_set {
            if !outside_leaves.contains(&l) && !grid.leaves.contains_key(&l) {
                interior_leaves.push(l);
            }
        }

        if !interior_leaves.is_empty() {
            for l_orig in interior_leaves {
                let mut leaf = LeafNode::new(l_orig, -bg);
                for offset in 0..LEAF_VOXELS {
                    leaf.values[offset] = -bg;
                    leaf.mask.set_on(offset);
                }
                grid.leaves.insert(l_orig, leaf);
            }
        }
    }

    // Check if the mesh is an open sheet (all voxels marked one side, or non-manifold)
    // If open sheet, ensure a solid thin shell is formed.
    let mut neg_count = 0;
    let mut pos_count = 0;
    for leaf in grid.leaves.values() {
        for offset in 0..LEAF_VOXELS {
            if leaf.mask.is_set(offset) {
                if leaf.values[offset] < 0.0 {
                    neg_count += 1;
                } else {
                    pos_count += 1;
                }
            }
        }
    }

    if neg_count == 0 && pos_count > 0 {
        // Open mesh: convert unsigned distance into a thin solid shell
        let shell_radius = 0.75 * voxel_size;
        for leaf in grid.leaves.values_mut() {
            for offset in 0..LEAF_VOXELS {
                if leaf.mask.is_set(offset) {
                    leaf.values[offset] -= shell_radius;
                }
            }
        }
    }

    grid
}
