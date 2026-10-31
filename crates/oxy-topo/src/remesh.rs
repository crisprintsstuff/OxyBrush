//! Voxel remeshing: rebuild a mesh (or the union of several meshes) as a
//! clean, uniformly dense surface.
//!
//! 1. Narrow-band unsigned distance to the nearest triangle on a regular grid
//!    (triangles rasterize in parallel into an atomic grid; each cell keeps
//!    the nearest distance and triangle id, packed into one `u64`).
//! 2. Sign: band cells use the nearest triangle's normal; a flood fill from
//!    the grid border marks the outside, so everything enclosed is inside.
//!    Open meshes (nothing enclosed) fall back to a thin shell.
//! 3. Surface nets extract the zero level set as quads, split into triangles.

use glam::{UVec3, Vec3};
use oxy_mesh::{Aabb, Mesh};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest supported resolution (grid memory grows with its cube).
pub const MAX_RESOLUTION: u32 = 320;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemeshOptions {
    /// Voxels along the longest bounding-box axis.
    pub resolution: u32,
    /// Laplacian relaxation passes on the result.
    pub smooth_passes: u32,
}

impl Default for RemeshOptions {
    fn default() -> Self {
        Self { resolution: 128, smooth_passes: 1 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemeshError {
    Empty,
    NoSurface,
}

impl std::fmt::Display for RemeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemeshError::Empty => write!(f, "nothing to remesh"),
            RemeshError::NoSurface => write!(f, "remesh produced no surface (try a higher resolution)"),
        }
    }
}

impl std::error::Error for RemeshError {}

struct Grid {
    origin: Vec3,
    h: f32,
    dims: UVec3,
}

impl Grid {
    fn index(&self, i: u32, j: u32, k: u32) -> usize {
        (i as usize) + self.dims.x as usize * (j as usize + self.dims.y as usize * k as usize)
    }

    fn point(&self, i: u32, j: u32, k: u32) -> Vec3 {
        self.origin + Vec3::new(i as f32, j as f32, k as f32) * self.h
    }

    fn len(&self) -> usize {
        self.dims.x as usize * self.dims.y as usize * self.dims.z as usize
    }
}

/// Remesh the union of `meshes`. Hidden triangles are included; masks and
/// face groups are reset.
pub fn voxel_remesh(meshes: &[&Mesh], opts: RemeshOptions) -> Result<Mesh, RemeshError> {
    let bounds = meshes.iter().fold(Aabb::EMPTY, |a, m| a.union(&m.bounds()));
    if bounds.is_empty() || meshes.iter().all(|m| m.triangle_count() == 0) {
        return Err(RemeshError::Empty);
    }
    let res = opts.resolution.clamp(8, MAX_RESOLUTION);
    let h = (bounds.extent().max_element() / res as f32).max(1e-6);
    let pad = 3.0;
    let origin = bounds.min - Vec3::splat(pad * h);
    let dims = ((bounds.extent() / h).ceil() + Vec3::splat(2.0 * pad + 1.0)).as_uvec3();
    let grid = Grid { origin, h, dims };

    // Union: each mesh gets its own signed distance field (so one mesh's
    // surface can't flip signs inside another), combined with `min`.
    let mut sdf = vec![f32::MAX; grid.len()];
    for m in meshes {
        let tris: Vec<[Vec3; 3]> = m.triangles.iter().map(|t| t.map(|i| m.positions[i as usize])).collect();
        if tris.is_empty() {
            continue;
        }
        let part = mesh_sdf(&grid, &tris);
        sdf.par_iter_mut().zip(part.par_iter()).for_each(|(a, &b)| *a = a.min(b));
    }

    // 3. Surface nets.
    let (positions, triangles) = surface_nets(&grid, &sdf);
    if triangles.is_empty() {
        return Err(RemeshError::NoSurface);
    }
    let mut mesh = Mesh::new(positions, triangles);
    for _ in 0..opts.smooth_passes {
        relax(&mut mesh, 0.5);
    }
    if opts.smooth_passes > 0 {
        mesh.refresh_all();
    }
    Ok(mesh)
}

/// Narrow-band signed distance for one triangle soup (band-clamped).
fn mesh_sdf(grid: &Grid, tris: &[[Vec3; 3]]) -> Vec<f32> {
    let (origin, h, dims) = (grid.origin, grid.h, grid.dims);
    // 1. Narrow-band distance + nearest triangle.
    let band = 2.0 * h;
    let cells: Vec<AtomicU64> = (0..grid.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    tris.par_iter().enumerate().for_each(|(ti, t)| {
        let lo = ((t[0].min(t[1]).min(t[2]) - Vec3::splat(band) - origin) / h).ceil().max(Vec3::ZERO).as_uvec3();
        let hi = ((t[0].max(t[1]).max(t[2]) + Vec3::splat(band) - origin) / h).floor().as_uvec3().min(dims - 1);
        for k in lo.z..=hi.z {
            for j in lo.y..=hi.y {
                for i in lo.x..=hi.x {
                    let p = grid.point(i, j, k);
                    let d = (closest_point_on_triangle(p, t[0], t[1], t[2]) - p).length();
                    if d <= band {
                        // Positive floats order like their bit patterns.
                        let key = ((d.to_bits() as u64) << 32) | ti as u64;
                        cells[grid.index(i, j, k)].fetch_min(key, Ordering::Relaxed);
                    }
                }
            }
        }
    });
    let cells: Vec<u64> = cells.into_iter().map(AtomicU64::into_inner).collect();

    // 2. Signs. Band cells: side of the nearest triangle.
    let band_sign = |idx: usize, p: Vec3| -> Option<(f32, bool)> {
        let key = cells[idx];
        if key == u64::MAX {
            return None;
        }
        let d = f32::from_bits((key >> 32) as u32);
        let t = &tris[(key & 0xffff_ffff) as usize];
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let c = closest_point_on_triangle(p, t[0], t[1], t[2]);
        Some((d, (p - c).dot(n) >= 0.0))
    };
    let mut sdf = vec![0f32; grid.len()];
    let mut passable = vec![false; grid.len()];
    sdf.par_chunks_mut(dims.x as usize * dims.y as usize)
        .zip(passable.par_chunks_mut(dims.x as usize * dims.y as usize))
        .enumerate()
        .for_each(|(k, (sdf_slice, pass_slice))| {
            for j in 0..dims.y {
                for i in 0..dims.x {
                    let local = (i + j * dims.x) as usize;
                    let idx = grid.index(i, j, k as u32);
                    match band_sign(idx, grid.point(i, j, k as u32)) {
                        Some((d, outside)) => {
                            sdf_slice[local] = d;
                            pass_slice[local] = outside;
                        }
                        None => {
                            sdf_slice[local] = band;
                            pass_slice[local] = true;
                        }
                    }
                }
            }
        });

    // Flood fill the outside from the border.
    let mut outside = vec![false; grid.len()];
    let mut stack: Vec<(u32, u32, u32)> = Vec::new();
    let push = |i: u32, j: u32, k: u32, outside: &mut Vec<bool>, stack: &mut Vec<(u32, u32, u32)>| {
        let idx = grid.index(i, j, k);
        if !outside[idx] && passable[idx] {
            outside[idx] = true;
            stack.push((i, j, k));
        }
    };
    for k in 0..dims.z {
        for j in 0..dims.y {
            for i in 0..dims.x {
                let border = i == 0 || j == 0 || k == 0 || i == dims.x - 1 || j == dims.y - 1 || k == dims.z - 1;
                if border {
                    push(i, j, k, &mut outside, &mut stack);
                }
            }
        }
    }
    while let Some((i, j, k)) = stack.pop() {
        if i > 0 {
            push(i - 1, j, k, &mut outside, &mut stack);
        }
        if j > 0 {
            push(i, j - 1, k, &mut outside, &mut stack);
        }
        if k > 0 {
            push(i, j, k - 1, &mut outside, &mut stack);
        }
        if i + 1 < dims.x {
            push(i + 1, j, k, &mut outside, &mut stack);
        }
        if j + 1 < dims.y {
            push(i, j + 1, k, &mut outside, &mut stack);
        }
        if k + 1 < dims.z {
            push(i, j, k + 1, &mut outside, &mut stack);
        }
    }

    let enclosed = outside.iter().any(|&o| !o);
    if enclosed {
        sdf.par_iter_mut().zip(outside.par_iter()).for_each(|(v, &o)| {
            if !o {
                *v = -*v;
            }
        });
    } else {
        // Open surface: extract a thin shell around it.
        let shell = 0.75 * h;
        sdf.par_iter_mut().for_each(|v| *v -= shell);
    }

    sdf
}

fn surface_nets(grid: &Grid, sdf: &[f32]) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let d = grid.dims;
    let cell_count_x = d.x - 1;
    let cell_count_y = d.y - 1;
    // Vertex per sign-changing cell, computed in parallel per z-slab.
    let corners: [UVec3; 8] =
        std::array::from_fn(|c| UVec3::new(c as u32 & 1, (c as u32 >> 1) & 1, (c as u32 >> 2) & 1));
    const EDGES: [(usize, usize); 12] =
        [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    let slabs: Vec<Vec<(u64, Vec3)>> = (0..d.z - 1)
        .into_par_iter()
        .map(|k| {
            let mut out = Vec::new();
            for j in 0..cell_count_y {
                for i in 0..cell_count_x {
                    let v: [f32; 8] = std::array::from_fn(|c| {
                        let o = corners[c];
                        sdf[grid.index(i + o.x, j + o.y, k + o.z)]
                    });
                    let neg = v.iter().filter(|&&x| x < 0.0).count();
                    if neg == 0 || neg == 8 {
                        continue;
                    }
                    let mut sum = Vec3::ZERO;
                    let mut n = 0;
                    for (a, b) in EDGES {
                        if (v[a] < 0.0) != (v[b] < 0.0) {
                            let t = v[a] / (v[a] - v[b]);
                            sum += corners[a].as_vec3().lerp(corners[b].as_vec3(), t);
                            n += 1;
                        }
                    }
                    let p = grid.point(i, j, k) + sum / n as f32 * grid.h;
                    out.push((cell_key(i, j, k), p));
                }
            }
            out
        })
        .collect();

    let mut positions = Vec::new();
    let mut index: HashMap<u64, u32> = HashMap::new();
    for slab in slabs {
        for (key, p) in slab {
            index.insert(key, positions.len() as u32);
            positions.push(p);
        }
    }

    // One quad per sign-changing grid edge, joining the 4 cells around it.
    let mut triangles = Vec::new();
    let cell = |i: u32, j: u32, k: u32| index.get(&cell_key(i, j, k)).copied();
    for k in 1..d.z - 1 {
        for j in 1..d.y - 1 {
            for i in 1..d.x - 1 {
                let v0 = sdf[grid.index(i, j, k)];
                for axis in 0..3 {
                    let (ni, nj, nk) = match axis {
                        0 => (i + 1, j, k),
                        1 => (i, j + 1, k),
                        _ => (i, j, k + 1),
                    };
                    if ni >= d.x || nj >= d.y || nk >= d.z {
                        continue;
                    }
                    let v1 = sdf[grid.index(ni, nj, nk)];
                    if (v0 < 0.0) == (v1 < 0.0) {
                        continue;
                    }
                    // The four cells sharing this edge, in a consistent cycle.
                    let quad = match axis {
                        0 => [cell(i, j - 1, k - 1), cell(i, j, k - 1), cell(i, j, k), cell(i, j - 1, k)],
                        1 => [cell(i - 1, j, k - 1), cell(i - 1, j, k), cell(i, j, k), cell(i, j, k - 1)],
                        _ => [cell(i - 1, j - 1, k), cell(i, j - 1, k), cell(i, j, k), cell(i - 1, j, k)],
                    };
                    let [Some(a), Some(b), Some(c), Some(e)] = quad else { continue };
                    // Orient so normals point from inside (negative) to outside.
                    let (a, b, c, e) = if v0 < 0.0 { (a, b, c, e) } else { (a, e, c, b) };
                    // Split along the shorter diagonal.
                    let (pa, pb, pc, pe) =
                        (positions[a as usize], positions[b as usize], positions[c as usize], positions[e as usize]);
                    if pa.distance_squared(pc) <= pb.distance_squared(pe) {
                        triangles.push([a, b, c]);
                        triangles.push([a, c, e]);
                    } else {
                        triangles.push([a, b, e]);
                        triangles.push([b, c, e]);
                    }
                }
            }
        }
    }
    (positions, triangles)
}

fn cell_key(i: u32, j: u32, k: u32) -> u64 {
    (i as u64) | ((j as u64) << 21) | ((k as u64) << 42)
}

/// One pass of Laplacian relaxation (positions only).
pub(crate) fn relax(mesh: &mut Mesh, amount: f32) {
    let prev = mesh.positions.clone();
    let m: &Mesh = mesh;
    let new: Vec<Vec3> = (0..m.vertex_count() as u32)
        .into_par_iter()
        .map(|v| {
            let n = m.neighbors(v);
            if n.is_empty() {
                return prev[v as usize];
            }
            let avg = n.iter().map(|&i| prev[i as usize]).sum::<Vec3>() / n.len() as f32;
            prev[v as usize].lerp(avg, amount)
        })
        .collect();
    mesh.positions = new;
}

/// Closest point on triangle (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_point_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn euler(m: &Mesh) -> i64 {
        let mut edges = std::collections::HashSet::new();
        for t in &m.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                edges.insert((a.min(b), a.max(b)));
            }
        }
        m.vertex_count() as i64 - edges.len() as i64 + m.triangle_count() as i64
    }

    #[test]
    fn sphere_remesh_is_closed_and_outward() {
        let s = primitives::icosphere(3, 1.0);
        let r = voxel_remesh(&[&s], RemeshOptions { resolution: 48, smooth_passes: 0 }).unwrap();
        assert_eq!(euler(&r), 2, "remeshed sphere should be a closed genus-0 surface");
        for (p, n) in r.positions.iter().zip(&r.normals) {
            assert!((p.length() - 1.0).abs() < 0.06, "vertex off surface: {}", p.length());
            assert!(p.dot(*n) > 0.0, "inward normal");
        }
    }

    #[test]
    fn union_of_overlapping_spheres() {
        let a = primitives::icosphere(3, 1.0);
        let mut d = primitives::icosphere(3, 1.0).to_data();
        d.positions.iter_mut().for_each(|p| *p += Vec3::X * 1.2);
        let b = Mesh::from_data(d);
        let r = voxel_remesh(&[&a, &b], RemeshOptions { resolution: 64, smooth_passes: 1 }).unwrap();
        assert_eq!(euler(&r), 2, "union should be one closed surface");
        // No vertex buried inside the other sphere.
        for p in &r.positions {
            assert!(p.length() > 0.9 || p.distance(Vec3::X * 1.2) > 0.9);
        }
    }

    #[test]
    fn open_plane_becomes_shell() {
        let p = primitives::plane(10, 2.0);
        let r = voxel_remesh(&[&p], RemeshOptions { resolution: 32, smooth_passes: 0 }).unwrap();
        assert!(r.triangle_count() > 100);
        assert!(r.bounds().extent().y < 0.3);
    }

    #[test]
    fn closest_point_regions() {
        let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
        assert_eq!(closest_point_on_triangle(Vec3::new(-1.0, -1.0, 0.0), a, b, c), a);
        let f = closest_point_on_triangle(Vec3::new(0.2, 0.2, 5.0), a, b, c);
        assert!(f.distance(Vec3::new(0.2, 0.2, 0.0)) < 1e-6);
        let e = closest_point_on_triangle(Vec3::new(0.5, -1.0, 0.0), a, b, c);
        assert!(e.distance(Vec3::new(0.5, 0.0, 0.0)) < 1e-6);
    }
}
