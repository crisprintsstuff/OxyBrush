//! Voxel remeshing: rebuild a mesh (or the union of several meshes) as a
//! clean, uniformly dense surface.
//!
//! 1. Narrow-band unsigned distance to the nearest triangle on a regular grid
//!    (triangles rasterize in parallel into an atomic grid; each cell keeps
//!    the nearest distance and triangle id, packed into one `u64`).
//! 2. Sign: band cells use the nearest triangle's normal; a flood fill from
//!    the grid border marks the outside, so everything enclosed is inside.
//!    Surfaces with holes have them patched first (see `patched`). With [`RemeshOptions::shell`], the signed distance is folded
//!    into a wall of the chosen thickness and side instead of being filled.
//!    Where parts of one mesh overlap (a primitive added into a piece, two
//!    pieces joined, a fold sculpted through the model), voxels next to the
//!    triangles buried inside the solid are signed by winding number
//!    instead ([`crate::winding`]): the buried surface used to count as a
//!    boundary, which left holes and thin walls.
//! 3. Surface nets extract the zero level set as quads, split into triangles.

use glam::{UVec3, Vec3};
use oxy_mesh::{Aabb, Mesh};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest supported resolution for dense regular grid mode.
pub const MAX_RESOLUTION: u32 = 320;

/// Volumetric engine used for remeshing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemeshEngine {
    /// Hierarchical sparse OpenVDB-style level set remesher (sparse leaf tree, low memory, scales past 1024).
    #[default]
    Vdb,
    /// Dense regular grid narrow-band remesher (legacy, capped at MAX_RESOLUTION).
    RegularGrid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemeshOptions {
    /// Voxels along the longest bounding-box axis.
    pub resolution: u32,
    /// Laplacian relaxation passes on the result.
    pub smooth_passes: u32,
    /// Adaptivity factor (0.0 = uniform grid; >0.0 simplifies flat regions).
    pub adaptivity: f32,
    /// Which volumetric engine to use.
    pub engine: RemeshEngine,
    /// Make a wall of this thickness around the surface instead of filling
    /// it (None: closed surfaces are filled, holes closed).
    pub shell: Option<Shell>,
}

impl Default for RemeshOptions {
    fn default() -> Self {
        Self { resolution: 128, smooth_passes: 1, adaptivity: 0.0, engine: RemeshEngine::Vdb, shell: None }
    }
}

/// A wall around the surface, for a remesh that makes a shell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shell {
    /// Wall thickness, in scene units.
    pub thickness: f32,
    pub side: ShellSide,
}

/// Which side of the surface the shell's wall goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellSide {
    /// Out from the surface (it stays the inside of the wall).
    Outward,
    /// Half on each side.
    #[default]
    Both,
    /// In from the surface (it stays the outside of the wall: a hollowed model).
    Inward,
}

impl ShellSide {
    pub const ALL: [ShellSide; 3] = [ShellSide::Outward, ShellSide::Both, ShellSide::Inward];

    pub fn label(self) -> &'static str {
        match self {
            ShellSide::Outward => "Outward",
            ShellSide::Both => "Both sides",
            ShellSide::Inward => "Inward",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ShellSide::Outward => "outward",
            ShellSide::Both => "both",
            ShellSide::Inward => "inward",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == k)
    }

    /// The shell's signed distance, from the surface's signed distance `d`
    /// (negative inside): negative in the wall.
    fn fold(self, d: f32, thickness: f32) -> f32 {
        match self {
            ShellSide::Outward => (-d).max(d - thickness),
            ShellSide::Both => d.abs() - thickness * 0.5,
            ShellSide::Inward => d.max(-d - thickness),
        }
    }
}

/// Options specifically for sparse VDB remeshing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VdbRemeshOptions {
    /// Voxel size in world units (if specified, overrides resolution).
    pub voxel_size: Option<f32>,
    /// Voxels along longest axis (used if voxel_size is None).
    pub resolution: u32,
    /// Adaptivity factor in [0.0, 1.0].
    pub adaptivity: f32,
    /// Laplacian relaxation passes.
    pub smooth_passes: u32,
    /// Surface offset dilation (>0) or erosion (<0).
    pub offset: f32,
    /// See [`RemeshOptions::shell`].
    pub shell: Option<Shell>,
}

impl Default for VdbRemeshOptions {
    fn default() -> Self {
        Self { voxel_size: None, resolution: 128, adaptivity: 0.0, smooth_passes: 1, offset: 0.0, shell: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemeshError {
    Empty,
    NoSurface,
    /// Nothing to fill: an open sheet, with Shell off.
    NotClosed,
}

impl std::fmt::Display for RemeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemeshError::Empty => write!(f, "nothing to remesh"),
            RemeshError::NoSurface => write!(f, "remesh produced no surface (try a higher resolution)"),
            RemeshError::NotClosed => {
                write!(f, "this surface doesn't enclose anything to fill: turn on Shell to give it a thickness")
            }
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
/// Vertex color, mask and face groups carry over from `meshes` (see
/// [`crate::transfer_attributes`]).
pub fn voxel_remesh(meshes: &[&Mesh], opts: RemeshOptions) -> Result<Mesh, RemeshError> {
    let copies = patched(meshes, opts.shell)?;
    let filled: Vec<&Mesh> = meshes.iter().zip(&copies).map(|(m, c)| c.as_ref().unwrap_or(m)).collect();
    let mut out = match opts.engine {
        RemeshEngine::Vdb => vdb_remesh(
            &filled,
            VdbRemeshOptions {
                voxel_size: None,
                resolution: opts.resolution,
                adaptivity: opts.adaptivity,
                smooth_passes: opts.smooth_passes,
                offset: 0.0,
                shell: opts.shell,
            },
        ),
        RemeshEngine::RegularGrid => regular_grid_remesh(&filled, opts),
    }?;
    crate::transfer_attributes(meshes, &mut out);
    Ok(out)
}

/// Calls `f` with the sign override for one mesh.
///
/// Why: the sign of a voxel comes from its nearest triangle. Where parts of
/// one mesh overlap (two spheres joined into one piece, a primitive added
/// with the Insert brush, a fold sculpted through the model), the triangles
/// buried inside the solid are nearest to voxels inside it and mark them
/// "outside", so the remesh got inner walls, holes and thin shells (issue
/// #174).
/// Those triangles, and two rings of triangles around them (which reach
/// across the lines where the surface crosses itself), get their side from
/// the winding number instead ([`crate::winding`]). `None` when nothing is
/// buried, which costs one winding query per 8 triangles.
fn with_sign_override<R>(positions: &[Vec3], triangles: &[[u32; 3]], f: impl FnOnce(&oxy_vdb::Signing) -> R) -> R {
    let winding = crate::winding::Winding::new(positions, triangles);
    let buried = winding.buried();
    if !buried.contains(&true) {
        return f(&oxy_vdb::Signing { thin_wall_if_open: false, ..Default::default() });
    }
    let suspect = crate::winding::grow_triangles(triangles, &buried, 2);
    let inside = |p: Vec3| winding.at(p) > 0.5;
    f(&oxy_vdb::Signing {
        sign: Some(oxy_vdb::SignOverride { suspect: &suspect, inside: &inside }),
        thin_wall_if_open: false,
    })
}

/// Without a shell, a mesh with holes has them patched first (as Mesh >
/// Close holes does), on a copy: a fill from outside would leak in through
/// them and leave only a thin wall around the surface. A surface that
/// encloses nothing (a sheet; see [`crate::winding::Winding::encloses`])
/// can't be filled.
///
/// RETURNS: per mesh, its patched copy (None: use it as it is).
fn patched(meshes: &[&Mesh], shell: Option<Shell>) -> Result<Vec<Option<Mesh>>, RemeshError> {
    meshes
        .iter()
        .map(|m| {
            if shell.is_some() || m.triangle_count() == 0 || !is_open(&m.triangles) {
                return Ok(None);
            }
            if !crate::winding::Winding::new(&m.positions, &m.triangles).encloses() {
                return Err(RemeshError::NotClosed);
            }
            let mut data = m.to_data();
            oxy_mesh::topo::close_holes(&mut data);
            Ok(Some(Mesh::from_data(data)))
        })
        .collect()
}

/// Whether the mesh has holes (or is an open surface): some edge belongs to
/// one triangle only. Edges are packed into `u64`s and sorted in parallel,
/// which is several times faster than a hash map on a million triangles.
fn is_open(triangles: &[[u32; 3]]) -> bool {
    let mut edges: Vec<u64> = triangles
        .par_iter()
        .flat_map_iter(|t| {
            (0..3).map(move |i| {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                ((a.min(b) as u64) << 32) | a.max(b) as u64
            })
        })
        .collect();
    edges.par_sort_unstable();
    let n = edges.len();
    (0..n).into_par_iter().any(|i| {
        let e = edges[i];
        (i == 0 || edges[i - 1] != e) && (i + 1 == n || edges[i + 1] != e)
    })
}

/// Fold a solid's level set into a shell of `shell`'s thickness and side.
fn fold_shell(grid: &mut oxy_vdb::VdbFloatGrid, shell: Shell) {
    let bg = grid.background;
    grid.leaves.par_iter_mut().for_each(|(_, leaf)| {
        for o in 0..oxy_vdb::LEAF_VOXELS {
            let v = shell.side.fold(leaf.values[o], shell.thickness).clamp(-bg, bg);
            leaf.values[o] = v;
            if v.abs() < bg {
                leaf.mask.set_on(o);
            }
        }
    });
}

/// The union of `meshes` as one narrow-band level set. Without a shell,
/// open meshes have their holes closed; with one, the band is wide enough
/// for the wall.
fn union_level_set(meshes: &[&Mesh], h: f32, shell: Option<Shell>) -> Result<oxy_vdb::VdbFloatGrid, RemeshError> {
    let half_width = shell.map_or(3.5, |s| (s.thickness / h + 3.0).max(3.5));
    let mut combined: Option<oxy_vdb::VdbFloatGrid> = None;
    for m in meshes {
        let (positions, triangles) = (&m.positions, &m.triangles);
        if triangles.is_empty() {
            continue;
        }
        let grid = with_sign_override(positions, triangles, |signing| {
            oxy_vdb::mesh_to_level_set_signed(
                positions,
                triangles,
                oxy_vdb::MeshToVolumeOptions { voxel_size: h, half_width },
                signing,
            )
        });
        match &mut combined {
            Some(target) => oxy_vdb::csg_union(target, &grid),
            None => combined = Some(grid),
        }
    }
    combined.ok_or(RemeshError::Empty)
}

/// Sparse OpenVDB-style hierarchical voxel remeshing.
pub fn vdb_remesh(meshes: &[&Mesh], opts: VdbRemeshOptions) -> Result<Mesh, RemeshError> {
    let bounds = meshes.iter().fold(Aabb::EMPTY, |a, m| a.union(&m.bounds()));
    if bounds.is_empty() || meshes.iter().all(|m| m.triangle_count() == 0) {
        return Err(RemeshError::Empty);
    }

    let h = opts.voxel_size.unwrap_or_else(|| {
        let res = opts.resolution.clamp(8, 2048);
        (bounds.extent().max_element() / res as f32).max(1e-5)
    });

    let mut grid = union_level_set(meshes, h, opts.shell)?;
    match opts.shell {
        Some(shell) => fold_shell(&mut grid, Shell { thickness: shell.thickness.max(h * 0.5), ..shell }),
        None => {
            let filled = grid
                .leaves
                .par_iter()
                .any(|(_, leaf)| (0..oxy_vdb::LEAF_VOXELS).any(|o| leaf.mask.is_set(o) && leaf.values[o] < 0.0));
            if !filled {
                return Err(RemeshError::NotClosed);
            }
        }
    }

    if opts.offset.abs() > 1e-5 {
        oxy_vdb::offset(&mut grid, opts.offset);
    }

    let mut mesh = oxy_vdb::volume_to_mesh(
        &grid,
        oxy_vdb::VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: opts.smooth_passes },
    )
    .ok_or(RemeshError::NoSurface)?;

    if opts.adaptivity > 0.0 {
        let decimate_ratio = (1.0 - opts.adaptivity * 0.7).clamp(0.2, 0.99);
        let target_tris = ((mesh.triangle_count() as f32 * decimate_ratio) as usize).max(4);
        mesh = crate::decimate(&mesh, target_tris);
    }

    Ok(mesh)
}

/// CSG volumetric Boolean operation between two meshes (Union, Difference, Intersection).
pub fn boolean_mesh(
    target: &Mesh,
    tool: &Mesh,
    op: oxy_vdb::BooleanOp,
    voxel_size: Option<f32>,
) -> Result<Mesh, RemeshError> {
    if target.triangle_count() == 0 || tool.triangle_count() == 0 {
        return Err(RemeshError::Empty);
    }

    let h = voxel_size.unwrap_or_else(|| {
        let extent = target.bounds().union(&tool.bounds()).extent();
        (extent.max_element() / 128.0).max(1e-5)
    });

    let mut target_grid = union_level_set(&[target], h, None)?;
    let tool_grid = union_level_set(&[tool], h, None)?;

    oxy_vdb::csg_operation(&mut target_grid, &tool_grid, op);

    let mut mesh = oxy_vdb::volume_to_mesh(
        &target_grid,
        oxy_vdb::VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 },
    )
    .ok_or(RemeshError::NoSurface)?;
    crate::transfer_attributes(&[target, tool], &mut mesh);
    Ok(mesh)
}

pub fn boolean_union(a: &Mesh, b: &Mesh) -> Result<Mesh, RemeshError> {
    boolean_mesh(a, b, oxy_vdb::BooleanOp::Union, None)
}

pub fn boolean_difference(target: &Mesh, tool: &Mesh) -> Result<Mesh, RemeshError> {
    boolean_mesh(target, tool, oxy_vdb::BooleanOp::Difference, None)
}

pub fn boolean_intersection(a: &Mesh, b: &Mesh) -> Result<Mesh, RemeshError> {
    boolean_mesh(a, b, oxy_vdb::BooleanOp::Intersection, None)
}

/// Dense regular-grid remesher (legacy narrow-band array).
pub fn regular_grid_remesh(meshes: &[&Mesh], opts: RemeshOptions) -> Result<Mesh, RemeshError> {
    let bounds = meshes.iter().fold(Aabb::EMPTY, |a, m| a.union(&m.bounds()));
    if bounds.is_empty() || meshes.iter().all(|m| m.triangle_count() == 0) {
        return Err(RemeshError::Empty);
    }
    let res = opts.resolution.clamp(8, MAX_RESOLUTION);
    let h = (bounds.extent().max_element() / res as f32).max(1e-6);
    // The band must reach past a shell's wall.
    let band = opts.shell.map_or(2.0 * h, |s| s.thickness.max(h * 0.5) + 2.0 * h);
    let pad = (band / h).ceil() + 1.0;
    let origin = bounds.min - Vec3::splat(pad * h);
    let dims = ((bounds.extent() / h).ceil() + Vec3::splat(2.0 * pad + 1.0)).as_uvec3();
    let grid = Grid { origin, h, dims };

    // Union: each mesh gets its own signed distance field (so one mesh's
    // surface can't flip signs inside another), combined with `min`.
    let mut sdf = vec![f32::MAX; grid.len()];
    for m in meshes {
        let (positions, triangles) = (&m.positions, &m.triangles);
        let tris: Vec<[Vec3; 3]> = triangles.iter().map(|t| t.map(|i| positions[i as usize])).collect();
        if tris.is_empty() {
            continue;
        }
        let (part, filled) =
            with_sign_override(positions, triangles, |signing| mesh_sdf(&grid, &tris, signing.sign.as_ref(), band));
        // A surface with holes whose fill reaches nothing (it leaks out, or
        // it's a sheet) can't be filled. (`voxel_remesh` patches holes
        // first; this is for direct callers.) A closed surface thinner than
        // the band is trusted: its sides come from the triangles' normals.
        if opts.shell.is_none() && !filled && is_open(triangles) {
            return Err(RemeshError::NotClosed);
        }
        sdf.par_iter_mut().zip(part.par_iter()).for_each(|(a, &b)| *a = a.min(b));
    }
    if let Some(shell) = opts.shell {
        let t = shell.thickness.max(h * 0.5);
        sdf.par_iter_mut().for_each(|v| *v = shell.side.fold(*v, t).min(band));
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
/// RETURNS: the distances, and whether any space beyond the band was enclosed.
fn mesh_sdf(grid: &Grid, tris: &[[Vec3; 3]], sign: Option<&oxy_vdb::SignOverride>, band: f32) -> (Vec<f32>, bool) {
    let (origin, h, dims) = (grid.origin, grid.h, grid.dims);
    // 1. Narrow-band distance + nearest triangle.
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
        let ti = (key & 0xffff_ffff) as usize;
        if let Some(s) = sign
            && s.suspect[ti]
        {
            // Inside next to a buried triangle is never on the surface (see oxy-vdb).
            let inside = (s.inside)(p);
            return Some((if inside { d.max(h * 1e-3) } else { d }, !inside));
        }
        let t = &tris[ti];
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

    // Filled: some space the fill could pass through (beyond the band, or
    // on a triangle's outer side) wasn't reached from outside. (A sheet's
    // back side is only band, which used to count as enclosed, so a sheet
    // came out as a slab two voxels thick.)
    let filled = outside.par_iter().zip(passable.par_iter()).any(|(&o, &p)| !o && p);
    let enclosed = outside.iter().any(|&o| !o);
    if enclosed {
        sdf.par_iter_mut().zip(outside.par_iter()).for_each(|(v, &o)| {
            if !o {
                *v = -*v;
            }
        });
    } else {
        // An open sheet (only reached with a shell, which folds it): each
        // side of it by its nearest triangle's normal.
        sdf.par_iter_mut().zip(passable.par_iter()).for_each(|(v, &out)| {
            if !out {
                *v = -*v;
            }
        });
    }

    (sdf, filled)
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
        let r = voxel_remesh(&[&s], RemeshOptions { resolution: 48, smooth_passes: 0, ..Default::default() }).unwrap();
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
        let r =
            voxel_remesh(&[&a, &b], RemeshOptions { resolution: 64, smooth_passes: 1, ..Default::default() }).unwrap();
        assert_eq!(euler(&r), 2, "union should be one closed surface");
        // No vertex buried inside the other sphere.
        for p in &r.positions {
            assert!(p.length() > 0.9 || p.distance(Vec3::X * 1.2) > 0.9);
        }
    }

    /// Two overlapping spheres in one mesh, as Join or the Insert brush
    /// make them; with `tied`, one sliver triangle joins them into one
    /// connected surface, as when a fold is sculpted through the model.
    fn joined_spheres(tied: bool) -> Mesh {
        let a = primitives::icosphere(4, 1.0).to_data();
        let mut d = a.clone();
        let n = a.positions.len() as u32;
        d.positions.extend(a.positions.iter().map(|p| *p + Vec3::X * 1.2));
        d.triangles.extend(a.triangles.iter().map(|t| t.map(|i| i + n)));
        if tied {
            // A small triangle where the two surfaces cross (the circle
            // x = 0.6, radius 0.8), from a point of one sphere to an edge
            // of the other.
            let p = Vec3::new(0.6, 0.8, 0.0);
            let nearest = |range: std::ops::Range<u32>| {
                range
                    .min_by(|&a, &b| {
                        d.positions[a as usize].distance(p).total_cmp(&d.positions[b as usize].distance(p))
                    })
                    .unwrap()
            };
            let (i, j) = (nearest(0..n), nearest(n..2 * n));
            let k = d.triangles.iter().skip(a.triangles.len()).find(|t| t.contains(&j)).unwrap();
            let k = *k.iter().find(|&&v| v != j).unwrap();
            d.triangles.push([i, j, k]);
        }
        d.mask = vec![0.0; d.positions.len()];
        d.groups = vec![0; d.triangles.len()];
        d.hidden = vec![false; d.triangles.len()];
        Mesh::from_data(d)
    }

    fn volume(m: &Mesh) -> f32 {
        m.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| m.positions[v as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn overlapping_parts_of_one_mesh_remesh_as_their_union() {
        // Issue #174: inner walls, holes and thin shells.
        let m = joined_spheres(false);
        // Union of two unit spheres 1.2 apart: 2 · 4/3 π − lens π(4 + 1.2)(2 − 1.2)² / 12.
        let expected = 2.0 * 4.0 / 3.0 * std::f32::consts::PI - std::f32::consts::PI * 5.2 * 0.64 / 12.0;
        for engine in [RemeshEngine::Vdb, RemeshEngine::RegularGrid] {
            let r = voxel_remesh(&[&m], RemeshOptions { resolution: 96, engine, ..Default::default() }).unwrap();
            assert_eq!(euler(&r), 2, "{engine:?}: one closed surface, no holes or inner walls");
            let v = volume(&r);
            assert!((v - expected).abs() / expected < 0.05, "{engine:?}: volume {v}, expected about {expected}");
            for p in &r.positions {
                assert!(p.length() > 0.9 || p.distance(Vec3::X * 1.2) > 0.9, "{engine:?}: point inside the union");
            }
        }
    }

    #[test]
    fn a_surface_passing_through_itself_remeshes_as_the_outside() {
        let m = joined_spheres(true);
        let expected = 2.0 * 4.0 / 3.0 * std::f32::consts::PI - std::f32::consts::PI * 5.2 * 0.64 / 12.0;
        for engine in [RemeshEngine::Vdb, RemeshEngine::RegularGrid] {
            let r = voxel_remesh(&[&m], RemeshOptions { resolution: 96, engine, ..Default::default() }).unwrap();
            let v = volume(&r);
            assert!((v - expected).abs() / expected < 0.05, "{engine:?}: volume {v}, expected about {expected}");
            assert_eq!(euler(&r), 2, "{engine:?}: one closed surface");
        }
    }

    #[test]
    fn a_hollow_shell_keeps_its_cavity() {
        // An outer sphere and an inner one facing inwards (a hollowed model):
        // the inner surface isn't buried, so the cavity stays.
        let outer = primitives::icosphere(4, 1.0).to_data();
        let inner = primitives::icosphere(4, 0.6).to_data();
        let mut d = outer.clone();
        let n = outer.positions.len() as u32;
        d.positions.extend(inner.positions);
        d.triangles.extend(inner.triangles.iter().map(|t| [t[0] + n, t[2] + n, t[1] + n]));
        d.mask = vec![0.0; d.positions.len()];
        d.groups = vec![0; d.triangles.len()];
        d.hidden = vec![false; d.triangles.len()];
        let m = Mesh::from_data(d);
        let expected = 4.0 / 3.0 * std::f32::consts::PI * (1.0 - 0.216);
        // (The regular-grid engine fills anything enclosed, cavities too.)
        let r = voxel_remesh(&[&m], RemeshOptions { resolution: 96, ..Default::default() }).unwrap();
        let v = volume(&r);
        assert!((v - expected).abs() / expected < 0.05, "volume {v}, expected about {expected}");
        assert_eq!(euler(&r), 4, "two closed surfaces");
    }

    #[test]
    fn an_open_sheet_needs_a_shell_and_gets_its_thickness() {
        let p = primitives::plane(10, 2.0);
        for engine in [RemeshEngine::Vdb, RemeshEngine::RegularGrid] {
            let opts = RemeshOptions { resolution: 64, smooth_passes: 0, engine, ..Default::default() };
            let Err(err) = voxel_remesh(&[&p], opts) else { panic!("{engine:?}: an open sheet was filled") };
            assert_eq!(err, RemeshError::NotClosed, "{engine:?}");
            // Both sides: a slab 0.2 thick, centred on the plane (y = 0).
            let both = Shell { thickness: 0.2, side: ShellSide::Both };
            let r = voxel_remesh(&[&p], RemeshOptions { shell: Some(both), ..opts }).unwrap();
            let b = r.bounds();
            assert!((b.extent().y - 0.2).abs() < 0.06, "{engine:?}: {}", b.extent().y);
            assert!(b.min.y.abs() - 0.1 < 0.04 && (b.min.y + b.max.y).abs() < 0.04, "{engine:?}: centred");
            assert_eq!(euler(&r), 2, "{engine:?}: one closed slab");
            // Outward: on the plane's front (its normal is +Y).
            let out = Shell { thickness: 0.2, side: ShellSide::Outward };
            let b = voxel_remesh(&[&p], RemeshOptions { shell: Some(out), ..opts }).unwrap().bounds();
            assert!(b.min.y > -0.05 && (b.max.y - 0.2).abs() < 0.05, "{engine:?}: {:?}", (b.min.y, b.max.y));
        }
    }

    #[test]
    fn holes_are_closed_without_a_shell() {
        // A sphere with a cap cut off remeshes as a whole ball (it used to
        // come out as a thin hollow wall).
        let d = primitives::icosphere(4, 1.0).to_data();
        let triangles: Vec<[u32; 3]> = d
            .triangles
            .iter()
            .copied()
            .filter(|t| t.iter().map(|&v| d.positions[v as usize].y).sum::<f32>() / 3.0 < 0.8)
            .collect();
        let n = triangles.len();
        let m = Mesh::new(d.positions.clone(), triangles);
        assert!(n < 5120, "a hole was cut");
        let ball = 4.0 / 3.0 * std::f32::consts::PI;
        for engine in [RemeshEngine::Vdb, RemeshEngine::RegularGrid] {
            let r = voxel_remesh(&[&m], RemeshOptions { resolution: 96, engine, ..Default::default() }).unwrap();
            if engine == RemeshEngine::Vdb {
                assert_eq!(euler(&r), 2, "{engine:?}");
            } else {
                // The older engine can leave a speck of a few points where
                // the patch meets the rim; the surface itself is one piece.
                assert!(euler(&r) <= 4, "{engine:?}: {}", euler(&r));
            }
            let v = volume(&r);
            // The cap above y = 0.8 is closed (nearly flat), so a little less than a ball.
            assert!(v > ball * 0.9 && v < ball * 1.02, "{engine:?}: volume {v}");
        }
    }

    #[test]
    fn a_shell_of_a_closed_model_is_hollow() {
        let s = primitives::icosphere(4, 1.0);
        let pi43 = 4.0 / 3.0 * std::f32::consts::PI;
        for (side, outer, inner) in
            [(ShellSide::Inward, 1.0f32, 0.8f32), (ShellSide::Outward, 1.2, 1.0), (ShellSide::Both, 1.1, 0.9)]
        {
            for engine in [RemeshEngine::Vdb, RemeshEngine::RegularGrid] {
                let opts = RemeshOptions {
                    resolution: 96,
                    engine,
                    shell: Some(Shell { thickness: 0.2, side }),
                    ..Default::default()
                };
                let r = voxel_remesh(&[&s], opts).unwrap();
                assert_eq!(euler(&r), 4, "{side:?} {engine:?}: an outer and an inner surface");
                let want = pi43 * (outer.powi(3) - inner.powi(3));
                let v = volume(&r);
                assert!((v - want).abs() / want < 0.08, "{side:?} {engine:?}: volume {v}, want {want}");
                let far = r.positions.iter().map(|p| p.length()).fold(0.0f32, f32::max);
                assert!((far - outer).abs() < 0.04, "{side:?} {engine:?}: outer radius {far}");
            }
        }
    }

    #[test]
    fn boolean_difference_cuts_hole() {
        let target = primitives::icosphere(3, 1.0);
        let mut d = primitives::icosphere(3, 0.6).to_data();
        d.positions.iter_mut().for_each(|p| *p += Vec3::X * 0.7);
        let tool = Mesh::from_data(d);

        let result = boolean_difference(&target, &tool).unwrap();
        assert!(result.vertex_count() > 50);
        assert!(result.triangle_count() > 50);
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
