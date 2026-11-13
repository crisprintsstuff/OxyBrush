//! Isosurface extraction from VDB level sets to polygon meshes (`volume_to_mesh`).
//!
//! Implements Dual Contouring (Ju, Losasso, Schaefer, Warren, "Dual Contouring of Hermite Data",
//! ACM SIGGRAPH 2002), with a vertex per surface piece in each cell as in Manifold Dual
//! Contouring (Schaefer, Ju, Warren, IEEE TVCG 2007), so close surfaces stay apart and the
//! result is a watertight quad and triangle mesh without pinched edges.

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

/// No vertex (in the per-edge tables of [`cell_pieces`]).
pub const NO_VERTEX: u8 = u8::MAX;

/// The pieces of surface in a dual cell whose 8 corner values are `vals`
/// (negative inside; corner `i` at `(i & 1, i >> 1 & 1, i >> 2 & 1)`), for
/// dual contouring with a vertex per piece (see step 1 of
/// [`volume_to_mesh`]). Only edges `a b` for which `use_edge` says yes are
/// counted.
///
/// RETURNS: one point per piece, the average of its edge crossings in cell
/// coordinates (0..=1 on each axis), and for each of the 12 cell edges
/// ([`CELL_EDGES`] order) which piece crosses it, or [`NO_VERTEX`].
pub fn cell_pieces(vals: &[f32; 8], use_edge: impl Fn(usize, usize) -> bool) -> CellPieces {
    cell_pieces_inner(vals, &use_edge)
}

/// The pieces of surface in one cell (see [`cell_pieces`]). A cell has at
/// most four (as many as marching cubes makes in a cube).
#[derive(Debug, Clone, Copy)]
pub struct CellPieces {
    /// One point per piece, in cell coordinates; the first `count` are used.
    pub points: [Vec3; MAX_PIECES],
    pub count: u8,
    /// For each of the 12 cell edges, the piece crossing it, or [`NO_VERTEX`].
    pub edges: [u8; 12],
}

/// The most pieces of surface a cell can hold.
pub const MAX_PIECES: usize = 4;

/// Whether one of the cell's faces is ambiguous (inside corners on one
/// diagonal, outside on the other). Only such faces can leave two sheets
/// sharing an edge (a tunnel through the cell), so a mesher checks for
/// pinches only around these cells (`oxy_mesh::topo::split_pinched_edges_among`).
pub fn has_ambiguous_face(vals: &[f32; 8]) -> bool {
    FACES.iter().any(|&[a, b, c, d]| {
        let i = |k: usize| vals[k] < 0.0;
        i(a) == i(c) && i(b) == i(d) && i(a) != i(b)
    })
}

fn cell_pieces_inner(vals: &[f32; 8], use_edge: &dyn Fn(usize, usize) -> bool) -> CellPieces {
    let group = corner_groups(vals);
    // Pieces: the pair of the inside and outside groups each separates
    // (`8 * in + out`), in the order first met, with their crossings' sum.
    let mut keys = [0u8; 12];
    let mut sums = [(Vec3::ZERO, 0u32); 12];
    let mut found = 0usize;
    let mut edge_piece = [NO_VERTEX; 12];
    for (e, (a, b)) in EDGES.into_iter().enumerate() {
        let (va, vb) = (vals[a], vals[b]);
        if (va < 0.0) != (vb < 0.0) && use_edge(a, b) {
            let t = va / (va - vb);
            let crossing = CORNERS[a].as_vec3().lerp(CORNERS[b].as_vec3(), t);
            let (inner, outer) = if va < 0.0 { (a, b) } else { (b, a) };
            let key = 8 * group[inner] + group[outer];
            let k = keys[..found].iter().position(|&x| x == key).unwrap_or_else(|| {
                keys[found] = key;
                found += 1;
                found - 1
            });
            sums[k].0 += crossing;
            sums[k].1 += 1;
            edge_piece[e] = k as u8;
        }
    }
    // One vertex per piece. (Never more than four; were there more, the
    // extra ones would share the last.)
    let mut points = [Vec3::ZERO; MAX_PIECES];
    let mut weights = [0u32; MAX_PIECES];
    for (k, &(sum, n)) in sums.iter().enumerate().take(found) {
        let slot = k.min(MAX_PIECES - 1);
        points[slot] += sum;
        weights[slot] += n;
    }
    let count = found.min(MAX_PIECES);
    for (p, &w) in points.iter_mut().zip(&weights).take(count) {
        *p /= w as f32;
    }
    let edges = edge_piece.map(|k| if k == NO_VERTEX { NO_VERTEX } else { k.min(MAX_PIECES as u8 - 1) });
    CellPieces { points, count: count as u8, edges }
}

/// The cell edge, in [`CELL_EDGES`] order, that a grid edge from cell corner
/// `from` (offset 0 or 1 per axis) one step along an axis is.
pub fn cell_edge(from: IVec3, step: IVec3) -> usize {
    edge_index(corner_index(from), corner_index(from + step))
}

/// The 12 edges of a cell as corner pairs.
pub const CELL_EDGES: [(usize, usize); 12] = EDGES;

/// A cell's vertices: one per piece of surface in it, and for each of its
/// 12 edges which of them (`NO_VERTEX` if the surface doesn't cross it).
struct CellVertices {
    key: u64,
    base: IVec3,
    pieces: CellPieces,
    /// The cell has an ambiguous face (where pinches can be left).
    ambiguous: bool,
}

/// Corner offsets of a dual cell: corner `i` is `(i & 1, i >> 1 & 1, i >> 2 & 1)`.
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

/// The six faces of a cell, each as its corners in order around it.
const FACES: [[usize; 4]; 6] = [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]];

fn corner_index(o: IVec3) -> usize {
    (o.x + 2 * o.y + 4 * o.z) as usize
}

fn edge_index(a: usize, b: usize) -> usize {
    EDGES.iter().position(|&(x, y)| (x, y) == (a.min(b), a.max(b))).expect("a cell edge")
}

/// Which corners of a cell are joined (see step 1 of [`volume_to_mesh`]):
/// for each corner, the lowest corner of its group. Inside corners join
/// along edges, and across an ambiguous face when its saddle is inside;
/// outside corners likewise, when it's outside. A piece of surface is then
/// a pair of an inside and an outside group (an inside slab between two
/// separate outside regions has two sheets, and gets two vertices).
fn corner_groups(vals: &[f32; 8]) -> [u8; 8] {
    let mut parent: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    fn root(p: &mut [u8; 8], mut x: u8) -> u8 {
        while p[x as usize] != x {
            x = p[x as usize];
        }
        x
    }
    let join = |p: &mut [u8; 8], a: usize, b: usize| {
        let (ra, rb) = (root(p, a as u8), root(p, b as u8));
        p[ra.max(rb) as usize] = ra.min(rb);
    };
    let inside = |i: usize| vals[i] < 0.0;
    for (a, b) in EDGES {
        if inside(a) == inside(b) {
            join(&mut parent, a, b);
        }
    }
    for [a, b, c, d] in FACES {
        // Ambiguous: inside on one diagonal, outside on the other.
        let diag = |x: usize, y: usize, u: usize, v: usize| inside(x) && inside(y) && !inside(u) && !inside(v);
        let (x, y, u, v) = if diag(a, c, b, d) {
            (a, c, b, d)
        } else if diag(b, d, a, c) {
            (b, d, a, c)
        } else {
            continue;
        };
        // Bilinear saddle value of the face (Nielson & Hamann 1991).
        let (va, vb, vc, vd) = (vals[a], vals[b], vals[c], vals[d]);
        let denom = va + vc - vb - vd;
        let saddle = if denom.abs() > f32::EPSILON { (va * vc - vb * vd) / denom } else { 0.0 };
        if saddle < 0.0 {
            join(&mut parent, x, y);
        } else {
            join(&mut parent, u, v);
        }
    }
    let mut out = [0u8; 8];
    for (i, o) in out.iter_mut().enumerate() {
        *o = root(&mut parent, i as u8);
    }
    out
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

    // 1. Find all cells in the active leaves containing an isosurface sign
    // change, with one vertex per separate piece of surface in the cell.
    //
    // A cell holding a single vertex for everything (plain dual contouring)
    // pinches two surfaces that pass through the same cell, such as the two
    // sides of a gap narrower than about two voxels between fingers, into
    // one vertex: the result has edges shared by four faces, joining parts
    // that should stay apart. Following Manifold Dual Contouring (Schaefer,
    // Ju & Warren, "Manifold Dual Contouring", IEEE TVCG 2007), each cell
    // gets one vertex per surface component of its marching-cubes patch,
    // and each sign-changing edge uses the vertex of the component it
    // belongs to. Components are groups of inside corners joined along cube
    // edges, and across a face where the face is ambiguous (inside corners
    // on one diagonal, outside on the other) only when the face's bilinear
    // saddle is inside: the asymptotic decider (Nielson & Hamann, "The
    // asymptotic decider", IEEE Visualization 1991). The decision depends
    // only on the face's four values, so the two cells sharing a face agree
    // and the surface stays closed.
    let leaves_vec: Vec<_> = grid.leaves.values().collect();

    // A voxel a whole band-width from the surface (the background value,
    // e.g. the filled inside of a solid) can't be the corner of a cell the
    // surface crosses: a cell is under two voxels across, and the band is
    // wider. Leaves made only of such voxels are skipped outright.
    let deep = grid.background.abs() * 0.999;
    let skip_deep = isoval.abs() < 1e-6 && deep > 0.0;
    let cells: Vec<CellVertices> = leaves_vec
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
                let mut pieces =
                    cell_pieces(&vals, |a, b| grid.is_active(base + CORNERS[a]) && grid.is_active(base + CORNERS[b]));
                if pieces.count == 0 {
                    continue;
                }
                for p in &mut pieces.points[..pieces.count as usize] {
                    *p = grid.transform.index_to_world_f(base.as_vec3() + *p);
                }
                let ambiguous = has_ambiguous_face(&vals);
                out.push(CellVertices { key: cell_key(base), base, pieces, ambiguous });
            }
            out
        })
        .collect();

    if cells.is_empty() {
        return None;
    }

    let mut positions = Vec::with_capacity(cells.len());
    // Per cell: its first vertex and which of its vertices each edge uses.
    let mut key_to_cell: HashMap<u64, (u32, [u8; 12])> =
        HashMap::with_capacity_and_hasher(cells.len(), Default::default());
    let mut active_coords = Vec::with_capacity(cells.len());
    let mut suspect: Vec<bool> = Vec::with_capacity(cells.len());

    for c in cells {
        if key_to_cell.contains_key(&c.key) {
            continue;
        }
        key_to_cell.insert(c.key, (positions.len() as u32, c.pieces.edges));
        let n = c.pieces.count as usize;
        suspect.extend(std::iter::repeat_n(c.ambiguous, n));
        positions.extend_from_slice(&c.pieces.points[..n]);
        active_coords.push(c.base);
    }

    // 2. Generate quads/triangles for each sign-changing grid edge, from the
    // vertex each of its four cells keeps for that edge.
    let cell_fn = |coord: IVec3, edge: usize| -> Option<u32> {
        let (first, map) = key_to_cell.get(&cell_key(coord))?;
        let k = map[edge];
        (k != NO_VERTEX).then(|| first + k as u32)
    };

    let mut triangles: Vec<[u32; 3]> = active_coords
        .par_iter()
        .flat_map_iter(|&base| {
            let mut triangles: Vec<[u32; 3]> = Vec::new();
            let v0 = grid.get_value(base) - isoval;

            // Check 3 outgoing edges from base: X (+1,0,0), Y (0,+1,0), Z (0,0,+1)
            for axis in 0..3 {
                let step = match axis {
                    0 => IVec3::new(1, 0, 0),
                    1 => IVec3::new(0, 1, 0),
                    _ => IVec3::new(0, 0, 1),
                };
                let neighbor_coord = base + step;

                let v1 = grid.get_value(neighbor_coord) - isoval;
                if (v0 < 0.0) == (v1 < 0.0) {
                    continue;
                }
                if !grid.is_active(base) || !grid.is_active(neighbor_coord) {
                    continue;
                }

                // The 4 cells sharing this edge
                let cells = match axis {
                    0 => [IVec3::new(0, -1, -1), IVec3::new(0, 0, -1), IVec3::ZERO, IVec3::new(0, -1, 0)],
                    1 => [IVec3::new(-1, 0, -1), IVec3::new(-1, 0, 0), IVec3::ZERO, IVec3::new(0, 0, -1)],
                    _ => [IVec3::new(-1, -1, 0), IVec3::new(0, -1, 0), IVec3::ZERO, IVec3::new(-1, 0, 0)],
                };
                // The grid edge, as seen from the cell at `base + o`.
                let quad = cells.map(|o| cell_fn(base + o, cell_edge(-o, step)));

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

    // Where a cell still couldn't tell two sheets apart (a tunnel through
    // it), separate them.
    oxy_mesh::topo::split_pinched_edges_among(&mut positions, &mut triangles, Some(&suspect));
    // Specks under two voxels across (a corner joined only through a cell's
    // middle) can't be part of the model: drop them.
    oxy_mesh::topo::drop_specks(&mut positions, &mut triangles, 2.0 * grid.transform.voxel_size);

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
