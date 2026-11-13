//! Conversion of polygon meshes into narrow-band VDB level-set grids (`mesh_to_level_set`).
//!
//! Signed distance computation uses angle-weighted vertex pseudonormals
//! (Bærentzen & Aanæs, "Signed Distance Computation Using the Angle Weighted Pseudonormal", IEEE TVCG 2005)
//! and closest-point triangle projections (Ericson, "Real-Time Collision Detection", 2005) followed by
//! flood-fill exterior signing.

use crate::tree::{LEAF_DIM, LEAF_VOXELS, LeafNode, Transform, VdbFloatGrid};
use glam::{IVec3, Vec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

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
    mesh_to_level_set_signed(positions, triangles, opts, &Signing::default())
}

/// How [`mesh_to_level_set_signed`] tells inside from outside.
pub struct Signing<'a> {
    /// Triangles whose side the winding number decides (see [`SignOverride`]).
    pub sign: Option<SignOverride<'a>>,
    /// When nothing comes out inside (an open sheet), make a thin wall about
    /// one voxel thick around the surface instead (the old behaviour; the
    /// remesh asks for a shell of its own thickness instead).
    pub thin_wall_if_open: bool,
}

impl Default for Signing<'_> {
    fn default() -> Self {
        Signing { sign: None, thin_wall_if_open: true }
    }
}

/// Which side of the surface a point is on, for the triangles where the
/// nearest triangle's normal can't tell: `suspect[t]` marks them, and
/// `inside(p)` answers for points whose nearest triangle is one.
///
/// Used for meshes that pass through themselves, where a triangle buried
/// inside the solid would otherwise mark the points around it as outside.
pub struct SignOverride<'a> {
    pub suspect: &'a [bool],
    pub inside: &'a (dyn Fn(Vec3) -> bool + Sync),
}

/// [`mesh_to_level_set`], with the sign of voxels next to the triangles in
/// `sign.suspect` decided by `sign.inside` instead of the nearest triangle's
/// pseudonormal.
pub fn mesh_to_level_set_signed(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    opts: MeshToVolumeOptions,
    signing: &Signing,
) -> VdbFloatGrid {
    let sign = signing.sign.as_ref();
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

    // 1. The 8×8×8 leaves each triangle touches (with a voxel to spare),
    //    grouped by a parallel sort, plus the leaves around them: the band
    //    (half_width voxels, under one leaf) never reaches further.
    let margin = voxel_size * 2.0;
    let pack = |l: IVec3| -> u64 {
        let k = (l / LEAF_DIM) + IVec3::splat(1 << 20);
        ((k.x as u64) << 42) | ((k.y as u64) << 21) | (k.z as u64)
    };
    let unpack = |k: u64| -> IVec3 {
        (IVec3::new((k >> 42) as i32, ((k >> 21) & 0x1F_FFFF) as i32, (k & 0x1F_FFFF) as i32) - IVec3::splat(1 << 20))
            * LEAF_DIM
    };
    let mut pairs: Vec<(u64, u32)> = triangles
        .par_iter()
        .enumerate()
        .flat_map_iter(|(ti, tri)| {
            let (p0, p1, p2) = (positions[tri[0] as usize], positions[tri[1] as usize], positions[tri[2] as usize]);
            let lo = VdbFloatGrid::leaf_origin(transform.world_to_index(p0.min(p1).min(p2) - Vec3::splat(margin)));
            let hi = VdbFloatGrid::leaf_origin(transform.world_to_index(p0.max(p1).max(p2) + Vec3::splat(margin)));
            let mut out = Vec::new();
            let mut z = lo.z;
            while z <= hi.z {
                let mut y = lo.y;
                while y <= hi.y {
                    let mut x = lo.x;
                    while x <= hi.x {
                        out.push((pack(IVec3::new(x, y, z)), ti as u32));
                        x += LEAF_DIM;
                    }
                    y += LEAF_DIM;
                }
                z += LEAF_DIM;
            }
            out
        })
        .collect();
    if pairs.is_empty() {
        return grid;
    }
    pairs.par_sort_unstable();
    let mut leaves_vec: Vec<(IVec3, Vec<usize>)> = Vec::new();
    for (k, t) in pairs {
        match leaves_vec.last_mut() {
            Some((o, v)) if pack(*o) == k => v.push(t as usize),
            _ => leaves_vec.push((unpack(k), vec![t as usize])),
        }
    }
    {
        let present: HashSet<IVec3> = leaves_vec.iter().map(|(o, _)| *o).collect();
        let mut extra: Vec<IVec3> = present
            .par_iter()
            .flat_map_iter(|o| {
                let o = *o;
                (0..27).map(move |k| o + IVec3::new(k % 3 - 1, (k / 3) % 3 - 1, k / 9 - 1) * LEAF_DIM)
            })
            .filter(|n| !present.contains(n))
            .collect();
        extra.par_sort_unstable_by_key(|v| (v.x, v.y, v.z));
        extra.dedup();
        leaves_vec.extend(extra.into_iter().map(|o| (o, Vec::new())));
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

    // 2. Narrow-band distances, in two stages (the usual narrow-band
    //    approach, e.g. Museth 2013 §5, "mesh to volume"):
    //    a. each triangle writes its exact distance into the voxels within
    //       about one voxel of it (a small box around the triangle, so a
    //       dense mesh costs a few voxels per triangle, not a whole leaf);
    //    b. the band grows outward a layer at a time: a voxel tests only the
    //       nearest triangles its 26 neighbours found. Distances out to one
    //       voxel are exact; further out they are the distance to one of the
    //       neighbours' triangles, which is how narrow-band methods work.
    //    Testing every triangle near a leaf against all 512 of its voxels
    //    (what this did before) cost 5 s for a 330k-triangle sculpt at any
    //    resolution.

    const NO_TRI: u32 = u32::MAX;
    let leaf_index: HashMap<IVec3, usize> = leaves_vec.iter().enumerate().map(|(i, (o, _))| (*o, i)).collect();
    // The 27 leaves around each leaf (itself included), by offset.
    let neighbours: Vec<[usize; 27]> = leaves_vec
        .iter()
        .map(|(o, _)| {
            let mut n = [usize::MAX; 27];
            for (k, slot) in n.iter_mut().enumerate() {
                let d = IVec3::new(k as i32 % 3 - 1, (k as i32 / 3) % 3 - 1, k as i32 / 9 - 1) * LEAF_DIM;
                if let Some(&i) = leaf_index.get(&(*o + d)) {
                    *slot = i;
                }
            }
            n
        })
        .collect();
    let n_leaves = leaves_vec.len();
    let mut dist = vec![f32::MAX; n_leaves * LEAF_VOXELS];
    let mut tri_of = vec![NO_TRI; n_leaves * LEAF_VOXELS];
    let exact = voxel_size * 1.0;

    // a. Exact distances near each triangle.
    dist.par_chunks_mut(LEAF_VOXELS).zip(tri_of.par_chunks_mut(LEAF_VOXELS)).zip(leaves_vec.par_iter()).for_each(
        |((dist, tri_of), (origin, tis))| {
            let leaf_max = *origin + IVec3::splat(LEAF_DIM - 1);
            for &ti in tis {
                let tri = &triangles[ti];
                let (v0, v1, v2) = (positions[tri[0] as usize], positions[tri[1] as usize], positions[tri[2] as usize]);
                // `world_to_index` rounds, so this covers every voxel within `exact`.
                let lo = transform.world_to_index(v0.min(v1).min(v2) - Vec3::splat(exact)).max(*origin);
                let hi = transform.world_to_index(v0.max(v1).max(v2) + Vec3::splat(exact)).min(leaf_max);
                for z in lo.z..=hi.z {
                    for y in lo.y..=hi.y {
                        for x in lo.x..=hi.x {
                            let local = IVec3::new(x, y, z) - *origin;
                            let off = (local.x | (local.y << 3) | (local.z << 6)) as usize;
                            let p = transform.index_to_world(IVec3::new(x, y, z));
                            let d = p.distance(closest_point_on_triangle(p, v0, v1, v2));
                            if d < dist[off] {
                                dist[off] = d;
                                tri_of[off] = ti as u32;
                            }
                        }
                    }
                }
            }
        },
    );

    // b. Grow the band from neighbours' nearest triangles. Each pass looks
    //    only at leaves next to a leaf that changed in the pass before, and
    //    reads its neighbourhood from a padded 10×10×10 copy.
    const PAD: usize = LEAF_DIM as usize + 2;
    let passes = 2 * (half_width.ceil() as usize) + 2;
    let mut changed_leaf: Vec<bool> = tri_of.par_chunks(LEAF_VOXELS).map(|c| c.iter().any(|&t| t != NO_TRI)).collect();
    // Voxels whose nearest triangle changed in the last pass: only their
    // triangles are new to their neighbours.
    let mut fresh: Vec<bool> = tri_of.par_iter().map(|&t| t != NO_TRI).collect();
    for _ in 0..passes {
        // A voxel's triangle as its neighbours see it: only if it is new.
        let snapshot: Vec<u32> =
            tri_of.par_iter().zip(fresh.par_iter()).map(|(&t, &f)| if f { t } else { NO_TRI }).collect();
        fresh.par_iter_mut().for_each(|f| *f = false);
        let active: Vec<bool> =
            neighbours.par_iter().map(|around| around.iter().any(|&n| n != usize::MAX && changed_leaf[n])).collect();
        changed_leaf = dist
            .par_chunks_mut(LEAF_VOXELS)
            .zip(tri_of.par_chunks_mut(LEAF_VOXELS))
            .zip(fresh.par_chunks_mut(LEAF_VOXELS))
            .enumerate()
            .map(|(li, ((dist, tri_of), fresh))| {
                if !active[li] {
                    return false;
                }
                let origin = leaves_vec[li].0;
                let around = &neighbours[li];
                // The leaf's voxels and a one-voxel border from its neighbours.
                let mut tile = [NO_TRI; PAD * PAD * PAD];
                for z in 0..PAD as i32 {
                    for y in 0..PAD as i32 {
                        for x in 0..PAD as i32 {
                            let q = IVec3::new(x - 1, y - 1, z - 1);
                            let l = IVec3::new(
                                q.x.div_euclid(LEAF_DIM),
                                q.y.div_euclid(LEAF_DIM),
                                q.z.div_euclid(LEAF_DIM),
                            );
                            let leaf = around[((l.x + 1) + (l.y + 1) * 3 + (l.z + 1) * 9) as usize];
                            if leaf != usize::MAX {
                                let r = q - l * LEAF_DIM;
                                tile[x as usize + PAD * (y as usize + PAD * z as usize)] =
                                    snapshot[leaf * LEAF_VOXELS + (r.x | (r.y << 3) | (r.z << 6)) as usize];
                            }
                        }
                    }
                }
                let mut changed = false;
                for off in 0..LEAF_VOXELS {
                    if dist[off] <= exact {
                        continue;
                    }
                    let (x, y, z) = (off & 7, (off >> 3) & 7, (off >> 6) & 7);
                    // The voxel and its six face neighbours (in the padded tile).
                    let c = (x + 1) + PAD * ((y + 1) + PAD * (z + 1));
                    let mut candidates = [NO_TRI; 7];
                    let mut count = 0;
                    for i in [c, c - 1, c + 1, c - PAD, c + PAD, c - PAD * PAD, c + PAD * PAD] {
                        let t = tile[i];
                        if t != NO_TRI && !candidates[..count].contains(&t) {
                            candidates[count] = t;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        continue;
                    }
                    let p = transform.index_to_world(origin + IVec3::new(x as i32, y as i32, z as i32));
                    for &t in &candidates[..count] {
                        if t == tri_of[off] {
                            continue;
                        }
                        let tri = &triangles[t as usize];
                        let (v0, v1, v2) =
                            (positions[tri[0] as usize], positions[tri[1] as usize], positions[tri[2] as usize]);
                        let d = p.distance(closest_point_on_triangle(p, v0, v1, v2));
                        // Beyond the band nothing is needed.
                        if d < dist[off] && d < bg {
                            dist[off] = d;
                            tri_of[off] = t;
                            fresh[off] = true;
                            changed = true;
                        }
                    }
                }
                changed
            })
            .collect();
        if !changed_leaf.iter().any(|c| *c) {
            break;
        }
    }

    // Signed distance of each band voxel: the side of its nearest
    // triangle, by the pseudonormal at the closest point. NaN beyond the band.
    let mut signed: Vec<f32> = vec![f32::NAN; n_leaves * LEAF_VOXELS];
    signed.par_chunks_mut(LEAF_VOXELS).enumerate().for_each(|(li, out)| {
        let origin = leaves_vec[li].0;
        for (off, o) in out.iter_mut().enumerate() {
            let t = tri_of[li * LEAF_VOXELS + off];
            if t == NO_TRI {
                continue;
            }
            let local = IVec3::new((off & 7) as i32, ((off >> 3) & 7) as i32, ((off >> 6) & 7) as i32);
            let p = transform.index_to_world(origin + local);
            let tri = &triangles[t as usize];
            let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (c, bary) = closest_point_and_barycentric(p, positions[i0], positions[i1], positions[i2]);
            let n = bary.x * vert_normals[i0] + bary.y * vert_normals[i1] + bary.z * vert_normals[i2];
            let diff = p - c;
            let d = dist[li * LEAF_VOXELS + off].min(bg);
            *o = match sign {
                // A point inside next to a buried triangle (even on it)
                // isn't on the outer surface: never 0, which would read as one.
                Some(s) if s.suspect[t as usize] => {
                    if (s.inside)(p) {
                        -d.max(voxel_size * 1e-3)
                    } else {
                        d
                    }
                }
                _ => {
                    let outside = diff.length_squared() <= 1e-10 || diff.dot(n) >= 0.0;
                    if outside { d } else { -d }
                }
            };
        }
    });

    // Voxels beyond the band take the side of a neighbour (the surface is
    // inside the band, so the side can't change out there). Only leaves that
    // reach the band are kept, and every part of such a leaf beyond the band
    // touches a band voxel of the same leaf, so this can stay within each leaf.
    signed.par_chunks_mut(LEAF_VOXELS).for_each(|out| {
        if !out.iter().any(|v| !v.is_nan()) {
            return;
        }
        loop {
            let mut changed = false;
            for off in 0..LEAF_VOXELS {
                if !out[off].is_nan() {
                    continue;
                }
                let (x, y, z) = (off & 7, (off >> 3) & 7, (off >> 6) & 7);
                let near = [
                    (x > 0).then(|| off - 1),
                    (x < 7).then(|| off + 1),
                    (y > 0).then(|| off - 8),
                    (y < 7).then(|| off + 8),
                    (z > 0).then(|| off - 64),
                    (z < 7).then(|| off + 64),
                ];
                if let Some(v) = near.into_iter().flatten().map(|n| out[n]).find(|v| !v.is_nan()) {
                    out[off] = if v < 0.0 { -bg } else { bg };
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    });

    // 3. Assemble the leaves that reach the band. Inside voxels are active
    //    (beyond the band at -background); outside ones beyond the band are not.
    for (li, (origin, _)) in leaves_vec.iter().enumerate() {
        let values = &signed[li * LEAF_VOXELS..(li + 1) * LEAF_VOXELS];
        if !values.iter().any(|v| v.abs() < bg) {
            continue;
        }
        let mut leaf = LeafNode::new(*origin, bg);
        for (offset, &v) in values.iter().enumerate() {
            let v = if v.is_nan() { bg } else { v };
            if v.abs() < bg || v < 0.0 {
                leaf.set(offset, v.clamp(-bg, bg));
            } else {
                leaf.values[offset] = v;
            }
        }
        grid.leaves.insert(*origin, leaf);
    }

    // 4. Fill enclosed interior leaves for solid closed meshes: flood the
    //    outside from the border of a box of leaves (a flat array, one byte
    //    per leaf), and every leaf it can't reach that isn't a band leaf is
    //    inside.
    let mut min_l = IVec3::splat(i32::MAX);
    let mut max_l = IVec3::splat(i32::MIN);
    for &orig in grid.leaves.keys() {
        min_l = min_l.min(orig);
        max_l = max_l.max(orig);
    }
    if min_l.x <= max_l.x {
        let box_min = min_l - IVec3::splat(LEAF_DIM);
        let box_max = max_l + IVec3::splat(LEAF_DIM);
        let dims = (box_max - box_min) / LEAF_DIM + IVec3::ONE;
        let (dx, dy, dz) = (dims.x as usize, dims.y as usize, dims.z as usize);
        let index = |l: IVec3| -> usize {
            let c = (l - box_min) / LEAF_DIM;
            c.x as usize + dx * (c.y as usize + dy * c.z as usize)
        };
        const FREE: u8 = 0;
        const BAND: u8 = 1;
        const OUTSIDE: u8 = 2;
        let mut state = vec![FREE; dx * dy * dz];
        for &l in grid.leaves.keys() {
            state[index(l)] = BAND;
        }
        // Start from the box's border (it is all outside: the box has a
        // leaf of padding).
        let mut queue: Vec<(usize, usize, usize)> = Vec::new();
        for z in 0..dz {
            for y in 0..dy {
                for x in 0..dx {
                    if x == 0 || y == 0 || z == 0 || x == dx - 1 || y == dy - 1 || z == dz - 1 {
                        let i = x + dx * (y + dy * z);
                        if state[i] == FREE {
                            state[i] = OUTSIDE;
                            queue.push((x, y, z));
                        }
                    }
                }
            }
        }
        while let Some((x, y, z)) = queue.pop() {
            let mut visit = |x: usize, y: usize, z: usize| {
                let i = x + dx * (y + dy * z);
                if state[i] == FREE {
                    state[i] = OUTSIDE;
                    queue.push((x, y, z));
                }
            };
            if x > 0 {
                visit(x - 1, y, z);
            }
            if x + 1 < dx {
                visit(x + 1, y, z);
            }
            if y > 0 {
                visit(x, y - 1, z);
            }
            if y + 1 < dy {
                visit(x, y + 1, z);
            }
            if z > 0 {
                visit(x, y, z - 1);
            }
            if z + 1 < dz {
                visit(x, y, z + 1);
            }
        }
        let interior: Vec<LeafNode> = state
            .par_iter()
            .enumerate()
            .filter(|(_, s)| **s == FREE)
            .map(|(i, _)| {
                let c = IVec3::new((i % dx) as i32, ((i / dx) % dy) as i32, (i / (dx * dy)) as i32);
                let mut leaf = LeafNode::new(box_min + c * LEAF_DIM, -bg);
                for offset in 0..LEAF_VOXELS {
                    leaf.values[offset] = -bg;
                    leaf.mask.set_on(offset);
                }
                leaf
            })
            .collect();
        grid.leaves.reserve(interior.len());
        for leaf in interior {
            grid.leaves.insert(leaf.origin, leaf);
        }
    }

    // Check if the mesh is an open sheet (all voxels marked one side, or non-manifold)
    // If open sheet, ensure a solid thin shell is formed.
    let any_active = |inside: bool| {
        grid.leaves
            .par_iter()
            .any(|(_, leaf)| (0..LEAF_VOXELS).any(|o| leaf.mask.is_set(o) && (leaf.values[o] < 0.0) == inside))
    };
    if signing.thin_wall_if_open && !any_active(true) && any_active(false) {
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
