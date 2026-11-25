//! Rebuild one Catmull–Clark coarse level under a dense all-quad mesh
//! ([RFC 0036](../../../docs/rfcs/0036-rebuild-lower-subdivision-level.md)).
//!
//! Connectivity recovery follows Taubin, "Detecting and reconstructing
//! subdivision connectivity", The Visual Computer 18:357–367 (2002): two
//! √Catmull–Clark inverses via bipartite vertex colouring on a closed
//! manifold quad mesh. Positions stay on the fine mesh; the recovered
//! coarse vertices take the fine V-vertex positions, and multiresolution
//! detail stores the difference from a fresh Catmull–Clark of that coarse
//! mesh (so sculpted high detail is kept).

use crate::multires::Multires;
use crate::{Mesh, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

/// Coarse faces, fine V-vertices, fine E-vertices keyed by fine V endpoints,
/// and fine F-vertices (same order as the coarse faces).
type RecoveredCc = (Vec<[u32; 4]>, Vec<u32>, HashMap<[u32; 2], u32>, Vec<u32>);

/// Coarse faces (indices into the V list), fine F-vertices, and fine V-vertices.
type IntermediateSplit = (Vec<[u32; 4]>, Vec<u32>, Vec<u32>);

/// Why a mesh cannot have a lower Catmull–Clark level rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildError {
    /// The piece already has subdivision levels.
    HasLevels,
    /// Not every face is a quad (or a recorded triangle pair).
    NotAllQuads,
    /// A boundary or non-manifold edge/vertex; the first version only
    /// recovers closed manifold meshes.
    NotClosedManifold,
    /// The connectivity is not exactly one Catmull–Clark step of a
    /// coarser mesh (ambiguous or missing).
    NoSubdivision,
    /// Vertex colours or UVs that the first version does not carry.
    UnsupportedAttributes,
}

impl RebuildError {
    pub fn message(self) -> &'static str {
        match self {
            Self::HasLevels => "This piece already has subdivision levels",
            Self::NotAllQuads => "Rebuild lower level needs an all-quad mesh",
            Self::NotClosedManifold => {
                "Rebuild lower level needs a closed manifold mesh (no holes or non-manifold edges)"
            }
            Self::NoSubdivision => "This mesh's connectivity is not a Catmull–Clark subdivision of a coarser mesh",
            Self::UnsupportedAttributes => {
                "Rebuild lower level cannot keep UVs or vertex colour yet; clear them or Keep this level only first"
            }
        }
    }
}

/// One Catmull–Clark level recovered under `mesh`: a two-level stack with
/// the input as the active high level.
///
/// PRECONDITION: `mesh` has no multires stack yet (the caller checks).
pub fn rebuild_lower_level(mesh: &Mesh) -> Result<(Mesh, Multires), RebuildError> {
    if !mesh.uvs().is_empty() || mesh.is_painted() {
        return Err(RebuildError::UnsupportedAttributes);
    }
    let quads = mesh_quads(mesh)?;
    let n = mesh.vertex_count();
    let (coarse_faces, v_of_coarse, e_of_fine, f_of_fine) = recover_cc(&quads, n)?;
    let coarse_n = v_of_coarse.len();
    let mut coarse_pos = vec![Vec3::ZERO; coarse_n];
    for (i, &v) in v_of_coarse.iter().enumerate() {
        coarse_pos[i] = mesh.positions[v as usize];
    }
    let coarse_tris = triangulate_quads(&coarse_faces);
    let coarse = Mesh::new(coarse_pos, coarse_tris);
    // Mesh::from_data reorders vertices; map original coarse indices → mesh indices.
    let src = coarse.source_vertices();
    let mut mesh_of = vec![0u32; src.len()];
    for (v, &s) in src.iter().enumerate() {
        mesh_of[s as usize] = v as u32;
    }
    let diags: HashSet<[u32; 2]> = coarse_faces
        .iter()
        .map(|q| {
            let a = mesh_of[q[0] as usize];
            let b = mesh_of[q[2] as usize];
            [a.min(b), a.max(b)]
        })
        .collect();
    coarse.set_quad_diagonals(diags);
    // Mark as recorded polygons so Catmull–Clark and later rebuilds keep quads.
    coarse.set_ngon_edges(HashSet::new());

    let mut mr = Multires::new_catmull_clark(&coarse, None);
    let prepared = mr.prepare(&coarse);
    let smooth = mr.add_level_prepared_with(prepared, true);
    if smooth.positions.len() != n {
        return Err(RebuildError::NoSubdivision);
    }
    let ordered = remap_fine_into_level_order(mesh, &mr, &v_of_coarse, &e_of_fine, &f_of_fine)?;
    mr.install_active_positions(ordered);
    let data = mr.active_mesh_data();
    let out = Mesh::from_data(data);
    mr.bind(&out);
    out.set_ngon_edges(HashSet::new());
    Ok((out, mr))
}

/// Place fine-mesh positions into Multires level-1 order (V, then E, then F).
fn remap_fine_into_level_order(
    mesh: &Mesh,
    mr: &Multires,
    v_of_coarse: &[u32],
    e_of_fine: &HashMap<[u32; 2], u32>,
    f_of_fine: &[u32],
) -> Result<Vec<Vec3>, RebuildError> {
    let (n_prev, edges) = mr.cc_edge_endpoints(1).ok_or(RebuildError::NoSubdivision)?;
    let n = mesh.vertex_count();
    if n_prev != v_of_coarse.len() || n_prev + edges.len() + f_of_fine.len() != n {
        return Err(RebuildError::NoSubdivision);
    }
    // Multires base vertices may be in a different order than `v_of_coarse`
    // (Mesh::from_data reorders). Match by position.
    let base = mr.base_positions();
    let mut fine_of_base = vec![u32::MAX; n_prev];
    let mut used = vec![false; v_of_coarse.len()];
    for (bi, &bp) in base.iter().enumerate() {
        let mut best = None;
        for (ci, &fv) in v_of_coarse.iter().enumerate() {
            if used[ci] {
                continue;
            }
            let d = mesh.positions[fv as usize].distance_squared(bp);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((ci, d));
            }
        }
        let Some((ci, d)) = best else {
            return Err(RebuildError::NoSubdivision);
        };
        if d > 1e-10 {
            return Err(RebuildError::NoSubdivision);
        }
        used[ci] = true;
        fine_of_base[bi] = v_of_coarse[ci];
    }

    let mut ordered = vec![Vec3::ZERO; n];
    for (bi, &fv) in fine_of_base.iter().enumerate() {
        ordered[bi] = mesh.positions[fv as usize];
    }
    for (e_idx, &(a, b)) in edges.iter().enumerate() {
        let fa = fine_of_base[a as usize];
        let fb = fine_of_base[b as usize];
        let Some(&fe) = e_of_fine.get(&edge_key(fa, fb)) else {
            return Err(RebuildError::NoSubdivision);
        };
        ordered[n_prev + e_idx] = mesh.positions[fe as usize];
    }
    // Face points: Multires face order may differ from recovery order. Match
    // each Multires face by the centroid of its coarse corners.
    let polys = mr.base_polygons().ok_or(RebuildError::NoSubdivision)?;
    let face_base = n_prev + edges.len();
    if polys.offsets.len() != f_of_fine.len() + 1 {
        return Err(RebuildError::NoSubdivision);
    }
    let mut f_used = vec![false; f_of_fine.len()];
    for f in 0..f_of_fine.len() {
        let start = polys.offsets[f] as usize;
        let end = polys.offsets[f + 1] as usize;
        if end - start != 4 {
            return Err(RebuildError::NoSubdivision);
        }
        let mut c = Vec3::ZERO;
        for &vi in &polys.corners[start..end] {
            c += base[vi as usize];
        }
        c /= 4.0;
        let mut best = None;
        for (fi, &ff) in f_of_fine.iter().enumerate() {
            if f_used[fi] {
                continue;
            }
            let d = mesh.positions[ff as usize].distance_squared(c);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((fi, d));
            }
        }
        let Some((fi, _)) = best else {
            return Err(RebuildError::NoSubdivision);
        };
        f_used[fi] = true;
        ordered[face_base + f] = mesh.positions[f_of_fine[fi] as usize];
    }
    Ok(ordered)
}

/// Edge key for undirected fine edges (fine vertex indices).
fn edge_key(a: u32, b: u32) -> [u32; 2] {
    [a.min(b), a.max(b)]
}

/// Quads of `mesh`. Prefers recorded diagonals (import POLY, cage, or
/// Multires::bind); geometric pairing alone often fails on non-planar
/// Catmull–Clark quads (RFC 0036).
fn mesh_quads(mesh: &Mesh) -> Result<Vec<[u32; 4]>, RebuildError> {
    let (q, t) = mesh.quad_and_triangle_counts();
    if t != 0 || q == 0 {
        return Err(RebuildError::NotAllQuads);
    }
    let diags = mesh.quad_diagonals_ref();
    let mut quads = Vec::with_capacity(q);
    let mut used = vec![false; mesh.triangle_count()];
    for (ti, tri) in mesh.triangles.iter().enumerate() {
        if used[ti] {
            continue;
        }
        let mut partner = None;
        for i in 0..3 {
            let a = tri[i];
            let b = tri[(i + 1) % 3];
            let k = edge_key(a, b);
            if !diags.contains(&k) {
                continue;
            }
            for (tj, other) in mesh.triangles.iter().enumerate() {
                if tj == ti || used[tj] {
                    continue;
                }
                let shares = (0..3)
                    .filter(|&j| {
                        let x = other[j];
                        let y = other[(j + 1) % 3];
                        edge_key(x, y) == k
                    })
                    .count();
                if shares == 1 {
                    partner = Some((tj, i));
                    break;
                }
            }
            if partner.is_some() {
                break;
            }
        }
        let Some((tj, diag_i)) = partner else {
            return Err(RebuildError::NotAllQuads);
        };
        used[ti] = true;
        used[tj] = true;
        let c = tri[(diag_i + 2) % 3];
        let other = mesh.triangles[tj];
        let d = other.iter().copied().find(|&v| v != tri[diag_i] && v != tri[(diag_i + 1) % 3]).unwrap();
        let a = tri[diag_i];
        let b = tri[(diag_i + 1) % 3];
        quads.push([a, d, b, c]);
    }
    if quads.len() != q {
        return Err(RebuildError::NotAllQuads);
    }
    Ok(quads)
}

fn triangulate_quads(quads: &[[u32; 4]]) -> Vec<[u32; 3]> {
    let mut tris = Vec::with_capacity(quads.len() * 2);
    for q in quads {
        tris.push([q[0], q[1], q[2]]);
        tris.push([q[0], q[2], q[3]]);
    }
    tris
}

/// Recover coarse quads and V/E/F maps from a closed manifold CC mesh.
///
/// RETURNS: coarse faces (coarse vertex indices), fine vertex of each coarse
/// vertex, fine E-vertex keyed by its two fine V endpoints, and fine F-vertex
/// per coarse face (same order as `coarse_faces`).
fn recover_cc(quads: &[[u32; 4]], n: usize) -> Result<RecoveredCc, RebuildError> {
    let (color, neighbors) = bipartite_color(quads, n)?;
    // Try each colour as the intermediate (V∪F); the other is E.
    for intermediate_color in [0u8, 1] {
        let e_color = 1 - intermediate_color;
        let Ok((coarse_faces, f_of_fine, v_of_inter)) = split_intermediate(n, &color, &neighbors, intermediate_color)
        else {
            continue;
        };
        // E map: each coarse edge's fine E-vertex is the shared e_color
        // neighbour of its two V endpoints.
        let mut e_of_fine = HashMap::new();
        let mut ok = true;
        for face in &coarse_faces {
            for i in 0..4 {
                let a = v_of_inter[face[i] as usize];
                let b = v_of_inter[face[(i + 1) % 4] as usize];
                let key = edge_key(a, b);
                if e_of_fine.contains_key(&key) {
                    continue;
                }
                let na = &neighbors[a as usize];
                let nb = &neighbors[b as usize];
                let common: Vec<u32> =
                    na.iter().copied().filter(|&u| color[u as usize] == e_color && nb.contains(&u)).collect();
                if common.len() != 1 {
                    ok = false;
                    break;
                }
                e_of_fine.insert(key, common[0]);
            }
            if !ok {
                break;
            }
        }
        if !ok {
            continue;
        }
        // Counts must match one Catmull–Clark step of an all-quad coarse mesh:
        // V' = V + E + F, F' = 4F (quads), E = 2F for a closed all-quad mesh.
        let v = v_of_inter.len();
        let f = coarse_faces.len();
        let e = e_of_fine.len();
        if v + e + f != n || e != 2 * f || quads.len() != 4 * f {
            continue;
        }
        return Ok((coarse_faces, v_of_inter, e_of_fine, f_of_fine));
    }
    Err(RebuildError::NoSubdivision)
}

/// Bipartite colouring of a closed manifold quad mesh, and ordered
/// neighbour rings (1-rings) for every vertex.
fn bipartite_color(quads: &[[u32; 4]], n: usize) -> Result<(Vec<u8>, Vec<Vec<u32>>), RebuildError> {
    let mut edge_faces: HashMap<[u32; 2], Vec<usize>> = HashMap::new();
    let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    for (fi, q) in quads.iter().enumerate() {
        let uniq: HashSet<u32> = q.iter().copied().collect();
        if uniq.len() != 4 || q.iter().any(|&v| v as usize >= n) {
            return Err(RebuildError::NotClosedManifold);
        }
        for i in 0..4 {
            let a = q[i];
            let b = q[(i + 1) % 4];
            edge_faces.entry(edge_key(a, b)).or_default().push(fi);
            neighbors[a as usize].insert(b);
            neighbors[b as usize].insert(a);
        }
    }
    for faces in edge_faces.values() {
        if faces.len() != 2 {
            return Err(RebuildError::NotClosedManifold);
        }
    }
    let mut ordered = vec![Vec::new(); n];
    let vert_quads: Vec<Vec<usize>> = {
        let mut vq = vec![Vec::new(); n];
        for (fi, q) in quads.iter().enumerate() {
            for &v in q {
                vq[v as usize].push(fi);
            }
        }
        vq
    };
    for v in 0..n {
        let ring = order_ring(v as u32, quads, &vert_quads[v])?;
        if ring.len() != neighbors[v].len() || ring.len() < 3 {
            return Err(RebuildError::NotClosedManifold);
        }
        ordered[v] = ring;
    }
    let mut color = vec![2u8; n];
    let mut queue = VecDeque::new();
    color[0] = 0;
    queue.push_back(0u32);
    while let Some(v) = queue.pop_front() {
        let c = color[v as usize];
        for &u in &ordered[v as usize] {
            if color[u as usize] == 2 {
                color[u as usize] = 1 - c;
                queue.push_back(u);
            } else if color[u as usize] == c {
                return Err(RebuildError::NoSubdivision);
            }
        }
    }
    if color.contains(&2) {
        return Err(RebuildError::NotClosedManifold);
    }
    Ok((color, ordered))
}

fn order_ring(v: u32, quads: &[[u32; 4]], incident: &[usize]) -> Result<Vec<u32>, RebuildError> {
    if incident.is_empty() {
        return Err(RebuildError::NotClosedManifold);
    }
    let mut next_along: HashMap<u32, u32> = HashMap::new();
    for &fi in incident {
        let q = &quads[fi];
        let i = q.iter().position(|&x| x == v).ok_or(RebuildError::NotClosedManifold)?;
        let prev = q[(i + 3) % 4];
        let next = q[(i + 1) % 4];
        if next_along.insert(prev, next).is_some() {
            return Err(RebuildError::NotClosedManifold);
        }
    }
    let start = *next_along.keys().next().ok_or(RebuildError::NotClosedManifold)?;
    let mut ring = vec![start];
    let mut at = next_along[&start];
    while at != start {
        if ring.len() > next_along.len() {
            return Err(RebuildError::NotClosedManifold);
        }
        ring.push(at);
        at = *next_along.get(&at).ok_or(RebuildError::NotClosedManifold)?;
    }
    if ring.len() != next_along.len() {
        return Err(RebuildError::NotClosedManifold);
    }
    Ok(ring)
}

/// Split the intermediate (V∪F) colour class into V and F via a second
/// √CC inverse on the subgraph of intermediate vertices.
///
/// Intermediate faces are the rings around each E-vertex. Coarse faces are
/// the rings of V-vertices around each F-vertex on that intermediate mesh
/// (not on the fine mesh — there an F only touches E).
///
/// RETURNS: coarse faces (indices into `v_of_inter`), fine F-vertices (same
/// order), and fine V-vertices.
fn split_intermediate(
    n: usize,
    color: &[u8],
    neighbors: &[Vec<u32>],
    intermediate_color: u8,
) -> Result<IntermediateSplit, RebuildError> {
    let e_color = 1 - intermediate_color;
    let mut inter_faces: Vec<Vec<u32>> = Vec::new();
    for v in 0..n {
        if color[v] != e_color {
            continue;
        }
        let ring = &neighbors[v];
        if ring.iter().any(|&u| color[u as usize] != intermediate_color) {
            return Err(RebuildError::NoSubdivision);
        }
        inter_faces.push(ring.clone());
    }
    let mut inter_verts: Vec<u32> = (0..n as u32).filter(|&v| color[v as usize] == intermediate_color).collect();
    inter_verts.sort_unstable();
    let local: HashMap<u32, u32> = inter_verts.iter().enumerate().map(|(i, &v)| (v, i as u32)).collect();
    let local_faces: Vec<[u32; 4]> = inter_faces
        .iter()
        .map(|face| {
            if face.len() != 4 {
                return Err(RebuildError::NoSubdivision);
            }
            Ok([local[&face[0]], local[&face[1]], local[&face[2]], local[&face[3]]])
        })
        .collect::<Result<_, _>>()?;
    let (inter_color, inter_neighbors) = bipartite_color(&local_faces, inter_verts.len())?;
    for v_local_color in [0u8, 1] {
        let f_local_color = 1 - v_local_color;
        let mut v_of_inter = Vec::new();
        let mut f_of_inter = Vec::new();
        for (li, &fine) in inter_verts.iter().enumerate() {
            if inter_color[li] == v_local_color {
                v_of_inter.push(fine);
            } else if inter_color[li] == f_local_color {
                f_of_inter.push(fine);
            }
        }
        if v_of_inter.is_empty() || f_of_inter.is_empty() {
            continue;
        }
        // Coarse faces from V-rings around each F on the intermediate mesh.
        let coarse_index: HashMap<u32, u32> = v_of_inter.iter().enumerate().map(|(i, &v)| (v, i as u32)).collect();
        let mut coarse_faces = Vec::with_capacity(f_of_inter.len());
        let mut ok = true;
        for &f in &f_of_inter {
            let lf = local[&f] as usize;
            let ring_local = &inter_neighbors[lf];
            if ring_local.len() != 4 || ring_local.iter().any(|&u| inter_color[u as usize] != v_local_color) {
                ok = false;
                break;
            }
            let mut face = [0u32; 4];
            for (i, &u) in ring_local.iter().enumerate() {
                let fine_v = inter_verts[u as usize];
                let Some(&ci) = coarse_index.get(&fine_v) else {
                    ok = false;
                    break;
                };
                face[i] = ci;
            }
            if !ok {
                break;
            }
            coarse_faces.push(face);
        }
        if ok {
            return Ok((coarse_faces, f_of_inter, v_of_inter));
        }
    }
    Err(RebuildError::NoSubdivision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multires::Scheme;
    use crate::primitives;
    use crate::retopo::{Cage, Face};

    fn cube_mesh() -> Mesh {
        let mut cage = Cage::default();
        let p = [
            Vec3::new(-0.5, -0.5, -0.5),
            Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(-0.5, -0.5, 0.5),
            Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(-0.5, 0.5, 0.5),
        ];
        cage.positions = p.to_vec();
        cage.faces = vec![
            Face::new(vec![0, 1, 2, 3]),
            Face::new(vec![4, 7, 6, 5]),
            Face::new(vec![0, 4, 5, 1]),
            Face::new(vec![1, 5, 6, 2]),
            Face::new(vec![2, 6, 7, 3]),
            Face::new(vec![3, 7, 4, 0]),
        ];
        cage.build_mesh().unwrap()
    }

    /// Dense CC level with recorded quads (as an import with POLY would have).
    fn dense_cc_cube() -> Mesh {
        let base = cube_mesh();
        let mut mr = Multires::new_catmull_clark(&base, None);
        let d = mr.add_level(&base);
        let fine = Mesh::from_data(d);
        mr.bind(&fine);
        // bind sets diagonals; mark n-gon edges so known_polygons is Some.
        fine.set_ngon_edges(HashSet::new());
        fine
    }

    #[test]
    fn rebuilds_one_level_under_a_catmull_clark_cube() {
        let fine = dense_cc_cube();
        assert_eq!(fine.quad_and_triangle_counts(), (24, 0));
        let (out, stack) = rebuild_lower_level(&fine).expect("recover");
        assert_eq!(stack.level_count(), 2);
        assert_eq!(stack.active(), 1);
        assert_eq!(stack.scheme(), Scheme::CatmullClark);
        assert_eq!(out.triangle_count(), fine.triangle_count());
        let mut expected = fine.positions.clone();
        expected.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(a.z.total_cmp(&b.z)));
        let mut got = out.positions.clone();
        got.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(a.z.total_cmp(&b.z)));
        for (a, b) in expected.iter().zip(&got) {
            assert!(a.distance(*b) < 1e-4, "{a} vs {b}");
        }
        let mut stack = stack;
        let d0 = stack.set_level(&out, 0);
        assert_eq!(d0.triangles.len(), 12); // cube: 6 quads → 12 tris
    }

    #[test]
    fn refuses_a_triangle_sphere() {
        let sphere = primitives::icosphere(2, 1.0);
        assert_eq!(rebuild_lower_level(&sphere).err(), Some(RebuildError::NotAllQuads));
    }

    #[test]
    fn refuses_an_open_plane() {
        let plane = primitives::plane(4, 2.0);
        // Plane may lack recorded polygons; either NotAllQuads or open-mesh.
        let err = rebuild_lower_level(&plane).err();
        assert!(
            matches!(
                err,
                Some(RebuildError::NotAllQuads)
                    | Some(RebuildError::NotClosedManifold)
                    | Some(RebuildError::NoSubdivision)
            ),
            "{err:?}"
        );
    }

    #[test]
    fn refuses_mesh_without_usable_quads() {
        let fine = dense_cc_cube();
        // Same triangles without recorded diagonals: geometric pairing fails
        // on non-planar Catmull–Clark quads.
        let bare = Mesh::new(fine.positions.clone(), fine.triangles.clone());
        assert_eq!(bare.quad_and_triangle_counts(), (0, 48));
        assert_eq!(rebuild_lower_level(&bare).err(), Some(RebuildError::NotAllQuads));
    }
}
