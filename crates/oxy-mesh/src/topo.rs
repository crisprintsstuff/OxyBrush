//! Topology operations that produce a new mesh.

use crate::Mesh;
use glam::Vec3;
use std::collections::HashMap;

/// Split every triangle into four at edge midpoints. Shared edges share midpoints.
pub fn midpoint_subdivide(positions: &[Vec3], tris: &[[u32; 3]]) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut pos = positions.to_vec();
    let mut edge_mid: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 3 / 2);
    let mut mid = |a: u32, b: u32, pos: &mut Vec<Vec3>| -> u32 {
        let key = (a.min(b), a.max(b));
        *edge_mid.entry(key).or_insert_with(|| {
            pos.push((pos[a as usize] + pos[b as usize]) * 0.5);
            pos.len() as u32 - 1
        })
    };
    let mut out = Vec::with_capacity(tris.len() * 4);
    for &[a, b, c] in tris {
        let ab = mid(a, b, &mut pos);
        let bc = mid(b, c, &mut pos);
        let ca = mid(c, a, &mut pos);
        out.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
    }
    (pos, out)
}

/// Undo pinches: where two sheets of surface share an edge (four triangles
/// on one edge, as a mesher makes where two surfaces come within a voxel of
/// each other), give each sheet its own copy of the edge's two points, so the
/// sheets are separate surfaces that happen to touch.
///
/// Around such an edge the four triangles alternate between bounding the
/// solid and bounding the outside. Triangles are paired across the solid
/// (each sheet is the two triangles that hold one solid part between them),
/// so the parts are kept apart rather than bridged. Then at each end point,
/// the triangles that are joined through shared edges (and these pairs) keep
/// the point; every other such group gets a copy.
///
/// Where the two sheets also meet around both ends of the edge, pairing
/// across the solid can't separate them; those edges are then paired across
/// the outside instead, which leaves a thin web between the parts but a
/// clean surface.
///
/// PRECONDITION: triangles are consistently oriented, outside to the left.
/// RETURNS: how many points were added. Edges shared by any other number of
/// triangles are left as they are.
pub fn split_pinched_edges(positions: &mut Vec<Vec3>, triangles: &mut [[u32; 3]]) -> usize {
    split_pinched_edges_among(positions, triangles, None)
}

/// [`split_pinched_edges`], looking only at edges whose two points are
/// both marked in `suspect` (a mesher knows where pinches can be, which
/// saves sorting every edge). Points added are not marked.
pub fn split_pinched_edges_among(
    positions: &mut Vec<Vec3>,
    triangles: &mut [[u32; 3]],
    suspect: Option<&[bool]>,
) -> usize {
    let suspect = suspect.map(|s| s.to_vec());
    let first = split_pinched(positions, triangles, true, suspect.as_deref());
    first + split_pinched(positions, triangles, false, suspect.as_deref())
}

fn split_pinched(
    positions: &mut Vec<Vec3>,
    triangles: &mut [[u32; 3]],
    across_solid: bool,
    suspect: Option<&[bool]>,
) -> usize {
    use rayon::prelude::*;
    let key = |a: u32, b: u32| ((a.min(b) as u64) << 32) | a.max(b) as u64;
    let marked = |v: u32| suspect.is_none_or(|s| s.get(v as usize).copied().unwrap_or(false));
    let mut keys: Vec<u64> = triangles
        .par_iter()
        .flat_map_iter(|t| {
            (0..3).filter_map(move |i| {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                (marked(a) && marked(b)).then(|| key(a, b))
            })
        })
        .collect();
    keys.par_sort_unstable();
    let mut pinched: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        let mut j = i;
        while j < keys.len() && keys[j] == keys[i] {
            j += 1;
        }
        if j - i == 4 {
            pinched.push(keys[i]);
        }
        i = j;
    }
    drop(keys);
    if pinched.is_empty() {
        return 0;
    }
    let pinched_set: std::collections::HashSet<u64> = pinched.iter().copied().collect();
    let mut ends: Vec<u32> = pinched.iter().flat_map(|&k| [(k >> 32) as u32, k as u32]).collect();
    ends.sort_unstable();
    ends.dedup();
    // Triangles around each end point.
    let mut around: HashMap<u32, Vec<u32>> = ends.iter().map(|&v| (v, Vec::new())).collect();
    for (t, tri) in triangles.iter().enumerate() {
        for &v in tri {
            if let Some(list) = around.get_mut(&v) {
                list.push(t as u32);
            }
        }
    }
    let mut added = 0;
    for x in ends {
        let tris = &around[&x];
        let n = tris.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let join = |p: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (root(p, a), root(p, b));
            p[ra.max(rb)] = ra.min(rb);
        };
        // Triangles (by local index) on each edge from x to y, and whether
        // they run x→y.
        let mut by_other: HashMap<u32, Vec<(usize, bool)>> = HashMap::new();
        for (k, &t) in tris.iter().enumerate() {
            let tri = triangles[t as usize];
            let at = tri.iter().position(|&v| v == x).expect("x in its triangle");
            let (next, prev) = (tri[(at + 1) % 3], tri[(at + 2) % 3]);
            by_other.entry(next).or_default().push((k, true));
            by_other.entry(prev).or_default().push((k, false));
        }
        for (y, list) in &by_other {
            match list.len() {
                2 => join(&mut parent, list[0].0, list[1].0),
                4 if pinched_set.contains(&key(x, *y)) => {
                    // Sort around the edge x→y by the angle of each
                    // triangle's third point.
                    let (px, py) = (positions[x as usize], positions[*y as usize]);
                    let d = (py - px).normalize_or_zero();
                    let u = d.any_orthonormal_vector();
                    let w = d.cross(u);
                    let mut sorted: Vec<(f32, usize, bool)> = list
                        .iter()
                        .map(|&(k, forward)| {
                            let tri = triangles[tris[k] as usize];
                            let third = *tri.iter().find(|&&v| v != x && v != *y).expect("a third point");
                            let r = positions[third as usize] - px;
                            (r.dot(w).atan2(r.dot(u)), k, forward)
                        })
                        .collect();
                    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
                    // The solid lies counter-clockwise of a triangle running
                    // y→x: pair it with the next one around.
                    for i in 0..4 {
                        let (a, b) = (sorted[i], sorted[(i + 1) % 4]);
                        if (!a.2 && b.2) == across_solid {
                            join(&mut parent, a.1, b.1);
                        }
                    }
                }
                _ => {}
            }
        }
        // The group of the first triangle keeps x; the others get copies.
        let mut copy: HashMap<usize, u32> = HashMap::new();
        let keep = root(&mut parent, 0);
        for k in 0..n {
            let r = root(&mut parent, k);
            if r == keep {
                continue;
            }
            let v = *copy.entry(r).or_insert_with(|| {
                positions.push(positions[x as usize]);
                added += 1;
                positions.len() as u32 - 1
            });
            for c in triangles[tris[k] as usize].iter_mut() {
                if *c == x {
                    *c = v;
                }
            }
        }
    }
    added
}

/// Remove specks: separate pieces whose bounding box is under `size` on
/// every axis (a mesher's leftovers, smaller than it can represent), and
/// the points only they used.
///
/// RETURNS: how many pieces were removed.
pub fn drop_specks(positions: &mut Vec<Vec3>, triangles: &mut Vec<[u32; 3]>, size: f32) -> usize {
    let n = positions.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn root(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    for t in triangles.iter() {
        for &v in &t[1..] {
            let (a, b) = (root(&mut parent, t[0]), root(&mut parent, v));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
    }
    let mut bounds: HashMap<u32, (Vec3, Vec3)> = HashMap::new();
    for t in triangles.iter() {
        let r = root(&mut parent, t[0]);
        let e = bounds.entry(r).or_insert((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)));
        for &v in t {
            e.0 = e.0.min(positions[v as usize]);
            e.1 = e.1.max(positions[v as usize]);
        }
    }
    let specks: std::collections::HashSet<u32> =
        bounds.iter().filter(|(_, (lo, hi))| (*hi - *lo).max_element() < size).map(|(&r, _)| r).collect();
    // Never remove everything.
    if specks.is_empty() || specks.len() == bounds.len() {
        return 0;
    }
    triangles.retain(|t| !specks.contains(&root(&mut parent, t[0])));
    // Keep only the points still used, in order.
    let mut used = vec![false; n];
    for t in triangles.iter() {
        for &v in t {
            used[v as usize] = true;
        }
    }
    let mut new_index = vec![u32::MAX; n];
    let mut kept = Vec::with_capacity(n);
    for v in 0..n {
        if used[v] {
            new_index[v] = kept.len() as u32;
            kept.push(positions[v]);
        }
    }
    *positions = kept;
    for t in triangles.iter_mut() {
        *t = t.map(|v| new_index[v as usize]);
    }
    specks.len()
}

/// Subdivide only part of a mesh: the triangles marked in `chosen` split
/// into four, and their neighbours split just enough (in two, or in four)
/// to meet the new points, so the surface stays closed: red-green
/// refinement (Bank, Sherman & Weiser 1983). A neighbour that would have
/// two of its edges split is split into four too, which keeps triangles
/// well shaped; that can spread the area by a little.
///
/// New points sit at edge middles, or with `smooth` on the interpolating
/// butterfly rule (Dyn, Levin & Gregory, "A butterfly subdivision scheme
/// for surface interpolation with tension control", ACM TOG 1990), which
/// curves them with the surface while every old point stays where it was,
/// so the rest of the mesh doesn't move. Edges at a border, or whose
/// neighbourhood is incomplete, use the middle.
///
/// Mask and color of new points are the average of the edge's ends; new
/// triangles keep their triangle's face group and visibility.
///
/// RETURNS: the new mesh data, or None if nothing is chosen.
pub fn subdivide_where(data: &crate::MeshData, chosen: &[bool], smooth: bool) -> Option<crate::MeshData> {
    let tris = &data.triangles;
    if !chosen.iter().any(|&c| c) {
        return None;
    }
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    // Triangles around each point (CSR), so the work stays near the chosen
    // area instead of hashing every edge of the mesh.
    let n = data.positions.len();
    let mut offsets = vec![0u32; n + 1];
    for tri in tris {
        for &v in tri {
            offsets[v as usize + 1] += 1;
        }
    }
    for i in 0..n {
        offsets[i + 1] += offsets[i];
    }
    let mut cursor = offsets.clone();
    let mut around = vec![0u32; tris.len() * 3];
    for (t, tri) in tris.iter().enumerate() {
        for &v in tri {
            around[cursor[v as usize] as usize] = t as u32;
            cursor[v as usize] += 1;
        }
    }
    let at = |v: u32| &around[offsets[v as usize] as usize..offsets[v as usize + 1] as usize];
    // The triangles on edge a b.
    let on_edge = |a: u32, b: u32| at(a).iter().copied().filter(move |&t| tris[t as usize].contains(&b));
    // The third point of the triangle with directed edge a → b.
    let opposite = |a: u32, b: u32| -> Option<u32> {
        at(a).iter().find_map(|&t| {
            let tri = tris[t as usize];
            let i = tri.iter().position(|&v| v == a)?;
            (tri[(i + 1) % 3] == b).then_some(tri[(i + 2) % 3])
        })
    };
    let edges_of = |t: usize| {
        let tri = tris[t];
        [key(tri[0], tri[1]), key(tri[1], tri[2]), key(tri[2], tri[0])]
    };
    // Grow the red set until every other triangle has at most one split edge.
    let mut red: Vec<bool> = (0..tris.len()).map(|t| chosen.get(t).copied().unwrap_or(false)).collect();
    let mut split: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut queue: Vec<(u32, u32)> = Vec::new();
    for t in (0..tris.len()).filter(|&t| red[t]) {
        for e in edges_of(t) {
            if split.insert(e) {
                queue.push(e);
            }
        }
    }
    while let Some((a, b)) = queue.pop() {
        // A neighbour across a newly split edge may now have two.
        for t in on_edge(a, b).collect::<Vec<_>>() {
            let t = t as usize;
            if red[t] {
                continue;
            }
            let es = edges_of(t);
            if es.iter().filter(|e| split.contains(e)).count() >= 2 {
                red[t] = true;
                for e in es {
                    if split.insert(e) {
                        queue.push(e);
                    }
                }
            }
        }
    }
    let p = &data.positions;
    let mut out = data.clone();
    let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
    let mut ordered: Vec<(u32, u32)> = split.into_iter().collect();
    ordered.sort_unstable();
    for (a, b) in ordered {
        let middle = (p[a as usize] + p[b as usize]) * 0.5;
        let butterfly = || -> Option<Vec3> {
            if on_edge(a, b).count() != 2 {
                return None;
            }
            let c = opposite(a, b)?;
            let d = opposite(b, a)?;
            let wings = [opposite(a, c)?, opposite(c, b)?, opposite(d, a)?, opposite(b, d)?];
            let e: Vec3 = wings.iter().map(|&w| p[w as usize]).sum();
            Some(middle + (p[c as usize] + p[d as usize]) * 0.125 - e * 0.0625)
        };
        let pos = if smooth { butterfly().unwrap_or(middle) } else { middle };
        let v = out.positions.len() as u32;
        out.positions.push(pos);
        if !data.mask.is_empty() {
            out.mask.push((data.mask[a as usize] + data.mask[b as usize]) * 0.5);
        }
        if !data.color.is_empty() {
            out.color.push((data.color[a as usize] + data.color[b as usize]) * 0.5);
        }
        mid.insert((a, b), v);
    }
    let m = |a: u32, b: u32| mid.get(&key(a, b)).copied();
    let (mut triangles, mut groups, mut hidden) = (Vec::new(), Vec::new(), Vec::new());
    for (t, &[a, b, c]) in tris.iter().enumerate() {
        let new: Vec<[u32; 3]> = match (m(a, b), m(b, c), m(c, a)) {
            (Some(ab), Some(bc), Some(ca)) => vec![[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]],
            (Some(ab), None, None) => vec![[a, ab, c], [ab, b, c]],
            (None, Some(bc), None) => vec![[a, b, bc], [a, bc, c]],
            (None, None, Some(ca)) => vec![[a, b, ca], [ca, b, c]],
            (None, None, None) => vec![[a, b, c]],
            _ => unreachable!("a triangle with two split edges was made red"),
        };
        for tri in new {
            triangles.push(tri);
            groups.push(data.groups.get(t).copied().unwrap_or(0));
            hidden.push(data.hidden.get(t).copied().unwrap_or(false));
        }
    }
    out.triangles = triangles;
    if !data.groups.is_empty() {
        out.groups = groups;
    }
    if !data.hidden.is_empty() {
        out.hidden = hidden;
    }
    Some(out)
}

/// Subdivide a mesh once, then apply `smooth_passes` of Laplacian smoothing
/// (a cheap stand-in for a true smooth subdivision scheme). Masks are carried over.
pub fn subdivide(mesh: &Mesh, smooth_passes: u32) -> Mesh {
    let (pos, tris) = midpoint_subdivide(&mesh.positions, &mesh.triangles);
    // Midpoints are appended after the original vertices; each gets the
    // larger mask of the original vertices it touches.
    let mut mask = mesh.mask.clone();
    mask.resize(pos.len(), 0.0);
    let n0 = mesh.vertex_count() as u32;
    for t in &tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a < n0 && b >= n0 {
                mask[b as usize] = mask[b as usize].max(mesh.mask[a as usize]);
            }
        }
    }
    // Midpoint colors are the average of their edge's end points, appended
    // in the order midpoint_subdivide creates them.
    let color = if mesh.is_painted() {
        let mut color = mesh.color.clone();
        let mut seen = std::collections::HashSet::with_capacity(mesh.triangles.len() * 3 / 2);
        for &[a, b, c] in &mesh.triangles {
            for (x, y) in [(a, b), (b, c), (c, a)] {
                if seen.insert((x.min(y), x.max(y))) {
                    color.push((mesh.color[x as usize] + mesh.color[y as usize]) * 0.5);
                }
            }
        }
        debug_assert_eq!(color.len(), pos.len());
        color
    } else {
        Vec::new()
    };
    // Each parent triangle becomes four consecutive children.
    let groups = mesh.groups().iter().flat_map(|&g| [g; 4]).collect();
    let hidden = mesh.hidden().iter().flat_map(|&h| [h; 4]).collect();
    let mut out = Mesh::from_data(crate::MeshData { positions: pos, triangles: tris, mask, groups, hidden, color });
    for _ in 0..smooth_passes {
        let prev = out.positions.clone();
        for v in 0..out.vertex_count() {
            let n = out.neighbors(v as u32);
            if n.is_empty() {
                continue;
            }
            let avg = n.iter().map(|&i| prev[i as usize]).sum::<Vec3>() / n.len() as f32;
            out.positions[v] = prev[v].lerp(avg, 0.5);
        }
    }
    if smooth_passes > 0 {
        out.refresh_all();
    }
    out
}

/// Diagnostics report on mesh manifoldness and edge topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifoldReport {
    /// True if the mesh is a closed watertight 2-manifold without boundary or non-manifold edges.
    pub is_manifold: bool,
    /// Number of open boundary edges (edges shared by only 1 face).
    pub boundary_edge_count: usize,
    /// Number of non-manifold edges (edges shared by > 2 faces).
    pub non_manifold_edge_count: usize,
    /// Number of degenerate triangles (triangles with duplicate vertex indices).
    pub degenerate_triangle_count: usize,
}

/// Checks the 2-manifold topology of `mesh`, counting boundary edges, non-manifold edges, and degenerate faces.
pub fn check_manifold(mesh: &Mesh) -> ManifoldReport {
    let mut boundary_edges = 0usize;
    let mut non_manifold_edges = 0usize;
    let mut degenerate_triangles = 0usize;

    let mut edge_counts: HashMap<(u32, u32), usize> = HashMap::with_capacity(mesh.triangle_count() * 3);

    for &[a, b, c] in &mesh.triangles {
        if a == b || b == c || c == a {
            degenerate_triangles += 1;
            continue;
        }
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let key = (u.min(v), u.max(v));
            *edge_counts.entry(key).or_insert(0) += 1;
        }
    }

    for &count in edge_counts.values() {
        if count == 1 {
            boundary_edges += 1;
        } else if count > 2 {
            non_manifold_edges += 1;
        }
    }

    let is_manifold = boundary_edges == 0 && non_manifold_edges == 0 && degenerate_triangles == 0;

    ManifoldReport {
        is_manifold,
        boundary_edge_count: boundary_edges,
        non_manifold_edge_count: non_manifold_edges,
        degenerate_triangle_count: degenerate_triangles,
    }
}

/// Helper to verify if two triangles sharing edge `(u, v)` form a valid convex quad.
fn is_valid_quad(positions: &[Vec3], t1: [u32; 3], t2: [u32; 3], u: u32, v: u32) -> bool {
    // Find the third vertex in each triangle
    let b = t1.iter().copied().find(|&x| x != u && x != v);
    let d = t2.iter().copied().find(|&x| x != u && x != v);
    let (Some(b), Some(d)) = (b, d) else { return false };
    if b == d || b == u || b == v || d == u || d == v {
        return false;
    }

    let pu = positions[u as usize];
    let pv = positions[v as usize];
    let pb = positions[b as usize];
    let pd = positions[d as usize];

    // Check normal consistency
    let n1 = (pb - pu).cross(pv - pu);
    let n2 = (pd - pv).cross(pu - pv);
    let l1 = n1.length();
    let l2 = n2.length();
    if l1 < 1e-6 || l2 < 1e-6 {
        return false;
    }
    if (n1 / l1).dot(n2 / l2) < 0.6 {
        return false;
    }

    // Check that b and d lie on opposite sides of edge uv
    let e_uv = pv - pu;
    let cross_b = (pb - pu).cross(e_uv);
    let cross_d = (pd - pu).cross(e_uv);
    if cross_b.dot(cross_d) >= 0.0 {
        return false;
    }

    // Check that u and v lie on opposite sides of diagonal bd
    let e_bd = pd - pb;
    let cross_u = (pu - pb).cross(e_bd);
    let cross_v = (pv - pb).cross(e_bd);
    if cross_u.dot(cross_v) >= 0.0 {
        return false;
    }

    // Corner angles at u and v must not be overly obtuse (rejects equilateral triangle pairs / diamonds)
    let len_ub = (pb - pu).length();
    let len_ud = (pd - pu).length();
    let len_vb = (pb - pv).length();
    let len_vd = (pd - pv).length();
    if len_ub < 1e-6 || len_ud < 1e-6 || len_vb < 1e-6 || len_vd < 1e-6 {
        return false;
    }
    let cos_u = (pb - pu).dot(pd - pu) / (len_ub * len_ud);
    let cos_v = (pb - pv).dot(pd - pv) / (len_vb * len_vd);
    if cos_u < -0.45 || cos_v < -0.45 {
        return false;
    }

    true
}

/// Find pairs of triangles that form convex, near-coplanar quads.
/// Returns a set of the shared diagonal edges `[min(u, v), max(u, v)]`.
pub fn find_quad_diagonals(positions: &[Vec3], triangles: &[[u32; 3]]) -> std::collections::HashSet<[u32; 2]> {
    let mut diagonals = std::collections::HashSet::new();
    let n = triangles.len();
    if n < 2 {
        return diagonals;
    }

    let mut used = vec![false; n];
    let mut consecutive_quads = 0;

    // Pass 1: Check consecutive pairs (as produced by OpenVDB dual contouring, surface nets, and primitives)
    for i in (0..n - 1).step_by(2) {
        let t1 = triangles[i];
        let t2 = triangles[i + 1];

        // Find shared vertices
        let mut shared = [0u32; 2];
        let mut count = 0;
        for &v1 in &t1 {
            if t2.contains(&v1) {
                if count < 2 {
                    shared[count] = v1;
                }
                count += 1;
            }
        }
        if count == 2 {
            let (u, v) = (shared[0], shared[1]);
            if is_valid_quad(positions, t1, t2, u, v) {
                diagonals.insert([u.min(v), u.max(v)]);
                used[i] = true;
                used[i + 1] = true;
                consecutive_quads += 1;
            }
        }
    }

    if consecutive_quads * 2 >= n.saturating_sub(1) {
        return diagonals;
    }

    // If a large mesh (>= 1000 triangles) has zero consecutive quads, it is a pure triangle
    // mesh (e.g., icosphere, STL, dynamic topology sculpt, subdivided triangles).
    // Avoid expensive pairing and avoid falsely turning equilateral triangles into quads.
    if consecutive_quads == 0 && n >= 1000 {
        return diagonals;
    }

    // Pass 2: Check any remaining unused triangles via sorted edge list (O(E log E) cache-friendly, no SipHash)
    let unused_count = n - consecutive_quads * 2;
    let mut edges: Vec<([u32; 2], u32)> = Vec::with_capacity(unused_count * 3);
    for (idx, &tri) in triangles.iter().enumerate() {
        if !used[idx] {
            for (u, v) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                edges.push(([u.min(v), u.max(v)], idx as u32));
            }
        }
    }

    use rayon::prelude::*;
    edges.par_sort_unstable_by_key(|e| e.0);

    let mut i = 0;
    while i + 1 < edges.len() {
        if edges[i].0 == edges[i + 1].0 {
            let key = edges[i].0;
            let t1_idx = edges[i].1 as usize;
            let t2_idx = edges[i + 1].1 as usize;
            if !used[t1_idx]
                && !used[t2_idx]
                && is_valid_quad(positions, triangles[t1_idx], triangles[t2_idx], key[0], key[1])
            {
                diagonals.insert(key);
                used[t1_idx] = true;
                used[t2_idx] = true;
            }
            while i < edges.len() && edges[i].0 == key {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    diagonals
}

/// Returns the number of paired quads and remaining standalone triangles in the mesh.
pub fn quad_and_triangle_counts(positions: &[Vec3], triangles: &[[u32; 3]]) -> (usize, usize) {
    let diagonals = find_quad_diagonals(positions, triangles);
    let quads = diagonals.len();
    let tris = triangles.len().saturating_sub(quads * 2);
    (quads, tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdivide_quadruples_triangles() {
        let m = crate::primitives::cube(2, 1.0);
        let s = subdivide(&m, 1);
        assert_eq!(s.triangle_count(), m.triangle_count() * 4);
        assert_ne!(s.topology_version(), m.topology_version());
    }

    #[test]
    fn manifold_check_cube_and_plane() {
        let cube = crate::primitives::cube(2, 1.0);
        let rep = check_manifold(&cube);
        assert!(rep.is_manifold);
        assert_eq!(rep.boundary_edge_count, 0);
        assert_eq!(rep.non_manifold_edge_count, 0);

        let plane = crate::primitives::plane(2, 1.0);
        let rep_plane = check_manifold(&plane);
        assert!(!rep_plane.is_manifold);
        assert!(rep_plane.boundary_edge_count > 0);
    }

    #[test]
    fn quad_detection_on_cube_grid() {
        let cube = crate::primitives::cube(2, 1.0);
        let (quads, tris) = quad_and_triangle_counts(&cube.positions, &cube.triangles);
        // 6 faces * 4 quads = 24 quads, 0 remaining standalone triangles
        assert_eq!(quads, 24);
        assert_eq!(tris, 0);
    }
}

/// The open boundaries of a mesh: each a closed loop of vertices, in the
/// order a filling triangle must run along it (opposite to the existing
/// triangles beside it), so a fill faces the same way as its neighbours.
///
/// Edges used by more than two triangles are not boundaries and are left
/// alone. A vertex where two holes touch is walked once per hole.
pub fn boundary_loops(triangles: &[[u32; 3]]) -> Vec<Vec<u32>> {
    let mut count: HashMap<(u32, u32), u32> = HashMap::with_capacity(triangles.len() * 3 / 2);
    for t in triangles {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    // Hole edges run b → a for a triangle edge a → b on the boundary.
    let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut edges = 0usize;
    for t in triangles {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            if count[&(a.min(b), a.max(b))] == 1 {
                next.entry(b).or_default().push(a);
                edges += 1;
            }
        }
    }
    let mut loops = Vec::new();
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut used = 0usize;
    for s in starts {
        while next.get(&s).is_some_and(|v| !v.is_empty()) && used < edges {
            let mut lp = vec![s];
            let mut v = s;
            while let Some(n) = next.get_mut(&v).and_then(|out| out.pop()) {
                used += 1;
                if n == s {
                    break;
                }
                lp.push(n);
                v = n;
                if lp.len() > edges {
                    break;
                }
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    loops
}

/// Close every hole in `data` with a patch of new triangles, and return how
/// many holes were closed.
///
/// Each hole is filled from its edge inward: rings of vertices stepping
/// toward the hole's centre, as many as the hole is wide in boundary edge
/// lengths, so the patch's triangles are about the size of the mesh's
/// around it. The new vertices are then relaxed (Laplacian smoothing with
/// the boundary fixed) so the patch sits smoothly in the gap. New triangles
/// get a new face group each, so a patch is easy to find, mask or hide;
/// new vertices are unmasked and keep the boundary's color.
///
/// INVARIANT: existing vertices and triangles are unchanged; only vertices
/// and triangles are appended.
pub fn close_holes(data: &mut crate::MeshData) -> usize {
    let loops = boundary_loops(&data.triangles);
    let first_group = data.groups.iter().copied().max().map_or(1, |g| g + 1);
    let has_color = !data.color.is_empty();
    for (group, lp) in (first_group..).zip(&loops) {
        let n = lp.len();
        let pts: Vec<Vec3> = lp.iter().map(|&v| data.positions[v as usize]).collect();
        let center = pts.iter().copied().sum::<Vec3>() / n as f32;
        let edge = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.distance(*b)).sum::<f32>() / n as f32;
        let radius = pts.iter().map(|p| p.distance(center)).sum::<f32>() / n as f32;
        let rings = ((radius / edge.max(1e-9)).round() as usize).clamp(1, 64);
        let color =
            if has_color { lp.iter().map(|&v| data.color[v as usize]).sum::<Vec3>() / n as f32 } else { Vec3::ONE };
        let first_new = data.positions.len() as u32;
        let push = |data: &mut crate::MeshData, p: Vec3| {
            data.positions.push(p);
            data.mask.push(0.0);
            if has_color {
                data.color.push(color);
            }
            data.positions.len() as u32 - 1
        };
        // Ring 0 is the boundary; rings 1..rings step inward; then the centre.
        let mut outer: Vec<u32> = lp.clone();
        let mut new_tris: Vec<[u32; 3]> = Vec::new();
        for r in 1..rings {
            let t = r as f32 / rings as f32;
            let inner: Vec<u32> = pts.iter().map(|&p| push(data, p.lerp(center, t))).collect();
            for i in 0..n {
                let j = (i + 1) % n;
                new_tris.push([outer[i], outer[j], inner[j]]);
                new_tris.push([outer[i], inner[j], inner[i]]);
            }
            outer = inner;
        }
        let c = push(data, center);
        for i in 0..n {
            new_tris.push([outer[i], outer[(i + 1) % n], c]);
        }
        // Relax the new vertices so the patch is smooth, the boundary fixed.
        let last_new = data.positions.len() as u32;
        let mut neighbors: HashMap<u32, Vec<u32>> = HashMap::new();
        for t in &new_tris {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                if a >= first_new {
                    neighbors.entry(a).or_default().push(b);
                }
                if b >= first_new {
                    neighbors.entry(b).or_default().push(a);
                }
            }
        }
        for _ in 0..(rings * 4).min(80) {
            let moved: Vec<(u32, Vec3)> = (first_new..last_new)
                .filter_map(|v| {
                    let ns = neighbors.get(&v)?;
                    let avg = ns.iter().map(|&u| data.positions[u as usize]).sum::<Vec3>() / ns.len() as f32;
                    Some((v, avg))
                })
                .collect();
            for (v, p) in moved {
                data.positions[v as usize] = p;
            }
        }
        data.groups.extend(std::iter::repeat_n(group, new_tris.len()));
        if !data.hidden.is_empty() {
            data.hidden.extend(std::iter::repeat_n(false, new_tris.len()));
        }
        data.triangles.extend(new_tris);
    }
    loops.len()
}

/// Make a mesh symmetric across the plane `position[axis] == center`:
/// keep the half on one side (`keep_positive`: the side where the
/// coordinate is larger), throw the other half away, and replace it with a
/// mirror image of the kept half, welded to it along the plane. Returns
/// `None` when nothing lies on the kept side.
///
/// Triangles that cross the plane are cut at it (polygon clipping against a
/// plane, as in Sutherland & Hodgman, "Reentrant polygon clipping", 1974),
/// so the seam is straight. Points cut from one edge are shared by both
/// triangles on that edge, and points lying on the plane (within a small
/// tolerance, then snapped onto it) are shared by both halves and merged
/// when several sit at the same place, so the result is welded: no seam,
/// no gap. Mask, vertex color, face groups and visibility are carried
/// across the cut (interpolated along cut edges) and mirrored with the half.
///
/// INVARIANT: every vertex of the result lies on the kept side or on the
/// plane, or is the mirror image of one that does; vertices on the plane
/// are not duplicated.
pub fn symmetrize(data: &crate::MeshData, axis: usize, center: f32, keep_positive: bool) -> Option<crate::MeshData> {
    let n = data.positions.len();
    let extent = {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in &data.positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        (hi - lo).max_element().max(1e-6)
    };
    let eps = extent * 1e-5;
    let sign = if keep_positive { 1.0 } else { -1.0 };
    // Signed distance to the plane, positive on the kept side; snapped to
    // exactly zero near it.
    let dist: Vec<f32> = data
        .positions
        .iter()
        .map(|p| {
            let d = (p[axis] - center) * sign;
            if d.abs() <= eps { 0.0 } else { d }
        })
        .collect();
    if !dist.iter().any(|&d| d > 0.0) {
        return None;
    }
    let has_mask = data.mask.len() == n;
    let has_color = data.color.len() == n;
    let mut out = crate::MeshData::default();
    // Vertices of the kept half: old vertices (by index) and cut points
    // (by edge), each added once.
    let mut remap = vec![u32::MAX; n];
    let mut on_plane: Vec<bool> = Vec::new();
    let add = |out: &mut crate::MeshData, on_plane: &mut Vec<bool>, p: Vec3, m: f32, c: Vec3, flat: bool| {
        out.positions.push(p);
        out.mask.push(m);
        if has_color {
            out.color.push(c);
        }
        on_plane.push(flat);
        out.positions.len() as u32 - 1
    };
    let mask = |v: usize| if has_mask { data.mask[v] } else { 0.0 };
    let color = |v: usize| if has_color { data.color[v] } else { Vec3::ONE };
    let mut keep_vertex = |v: u32, out: &mut crate::MeshData, on_plane: &mut Vec<bool>| -> u32 {
        let r = &mut remap[v as usize];
        if *r == u32::MAX {
            let mut p = data.positions[v as usize];
            let flat = dist[v as usize] == 0.0;
            if flat {
                p[axis] = center;
            }
            *r = add(out, on_plane, p, mask(v as usize), color(v as usize), flat);
        }
        *r
    };
    let mut cuts: HashMap<(u32, u32), u32> = HashMap::new();
    let mut groups = Vec::new();
    let mut hidden = Vec::new();
    for (t, tri) in data.triangles.iter().enumerate() {
        let d = tri.map(|v| dist[v as usize]);
        if d.iter().all(|&x| x <= 0.0) {
            continue; // removed side, or lying in the plane
        }
        // Clip the triangle to the kept side: walk its edges, keeping
        // corners on the kept side or the plane and adding a cut point where
        // an edge goes strictly from one side to the other.
        let mut poly: Vec<u32> = Vec::with_capacity(4);
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            let (da, db) = (d[i], d[(i + 1) % 3]);
            if da >= 0.0 {
                poly.push(keep_vertex(a, &mut out, &mut on_plane));
            }
            if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
                let key = (a.min(b), a.max(b));
                let idx = match cuts.get(&key) {
                    Some(&i) => i,
                    None => {
                        let f = da / (da - db);
                        let (ua, ub) = (a as usize, b as usize);
                        let mut p = data.positions[ua].lerp(data.positions[ub], f);
                        p[axis] = center;
                        let m = mask(ua) + (mask(ub) - mask(ua)) * f;
                        let c = color(ua).lerp(color(ub), f);
                        let i = add(&mut out, &mut on_plane, p, m, c, true);
                        cuts.insert(key, i);
                        i
                    }
                };
                poly.push(idx);
            }
        }
        for k in 1..poly.len().saturating_sub(1) {
            out.triangles.push([poly[0], poly[k], poly[k + 1]]);
            groups.push(data.groups.get(t).copied().unwrap_or(0));
            hidden.push(data.hidden.get(t).copied().unwrap_or(false));
        }
    }
    // Weld: points on the plane at the same place become one (an import may
    // have split vertices along the middle, for example at a UV seam).
    let q = |p: Vec3| ((p * (1.0 / eps)).round()).as_ivec3();
    let mut seen: HashMap<glam::IVec3, u32> = HashMap::new();
    let mut weld: Vec<u32> = (0..out.positions.len() as u32).collect();
    for v in 0..out.positions.len() {
        if on_plane[v] {
            weld[v] = *seen.entry(q(out.positions[v])).or_insert(v as u32);
        }
    }
    for t in &mut out.triangles {
        *t = t.map(|v| weld[v as usize]);
    }
    // Mirror the half: new vertices for those off the plane, shared ones on it.
    let half = out.positions.len();
    let mut mirror = vec![0u32; half];
    for v in 0..half {
        mirror[v] = if on_plane[v] {
            weld[v]
        } else {
            let mut p = out.positions[v];
            p[axis] = 2.0 * center - p[axis];
            out.positions.push(p);
            out.mask.push(out.mask[v]);
            if has_color {
                out.color.push(out.color[v]);
            }
            out.positions.len() as u32 - 1
        };
    }
    let kept = out.triangles.len();
    for t in 0..kept {
        let [a, b, c] = out.triangles[t];
        // Reversed winding keeps the mirrored faces facing outward.
        out.triangles.push([mirror[a as usize], mirror[c as usize], mirror[b as usize]]);
    }
    out.groups = groups.iter().chain(&groups).copied().collect();
    out.hidden = hidden.iter().chain(&hidden).copied().collect();
    // Points merged away by the weld are left unused; `Mesh::from_data`
    // drops them, along with any triangle the weld collapsed.
    Some(out)
}

#[cfg(test)]
mod symmetrize_tests {
    use super::*;
    use crate::{Mesh, MeshData, primitives};

    /// A sphere with a bump on the +X side, so the halves differ.
    fn lopsided() -> MeshData {
        let mut d = primitives::icosphere(3, 1.0).to_data();
        for p in &mut d.positions {
            if p.x > 0.5 {
                *p *= 1.3;
            }
        }
        d
    }

    fn assert_mirror_symmetric(m: &Mesh, axis: usize, center: f32) {
        let q = |p: Vec3| (p * 1e4).round().as_ivec3();
        let set: std::collections::HashSet<glam::IVec3> = m.positions.iter().map(|&p| q(p)).collect();
        for &p in &m.positions {
            let mut r = p;
            r[axis] = 2.0 * center - r[axis];
            assert!(set.contains(&q(r)), "{p} has no mirror image");
        }
    }

    #[test]
    fn symmetrize_mirrors_the_kept_half_and_welds_the_seam() {
        let d = lopsided();
        for (center, keep_positive) in [(0.0, true), (0.0, false), (0.13, true), (-0.21, false)] {
            let out = symmetrize(&d, 0, center, keep_positive).expect("something on the kept side");
            let m = Mesh::from_data(out);
            assert_mirror_symmetric(&m, 0, center);
            // Still a closed surface: the halves are welded along the plane.
            assert!(boundary_loops(&m.triangles).is_empty(), "open seam at {center} {keep_positive}");
            // The kept side is unchanged: the bump is on both sides or neither.
            let max_x = m.positions.iter().map(|p| (p.x - center).abs()).fold(0.0f32, f32::max);
            // Unbumped, the kept half reaches 1.0 from the origin; bumped, 1.3.
            let bumped = max_x > if keep_positive { 1.15 - center } else { 1.15 + center };
            assert_eq!(bumped, keep_positive, "{center} {keep_positive}: max |x| {max_x}");
            // No point on the plane is duplicated.
            let on: Vec<Vec3> = m.positions.iter().copied().filter(|p| (p.x - center).abs() < 1e-5).collect();
            let mut keys: Vec<_> = on.iter().map(|p| (p * 1e4).round().as_ivec3().to_array()).collect();
            keys.sort_unstable();
            let n = keys.len();
            keys.dedup();
            assert_eq!(keys.len(), n, "seam points welded");
        }
        // The other axes work the same way.
        for axis in [1, 2] {
            let m = Mesh::from_data(symmetrize(&d, axis, 0.05, true).unwrap());
            assert_mirror_symmetric(&m, axis, 0.05);
            assert!(boundary_loops(&m.triangles).is_empty());
        }
    }

    #[test]
    fn symmetrize_carries_mask_color_and_groups_across() {
        let mut d = lopsided();
        let n = d.positions.len();
        d.mask = d.positions.iter().map(|p| if p.x > 0.3 { 1.0 } else { 0.0 }).collect();
        d.color = d.positions.iter().map(|p| if p.y > 0.0 { Vec3::X } else { Vec3::ONE }).collect();
        d.groups = d.triangles.iter().map(|t| u32::from(d.positions[t[0] as usize].z > 0.0)).collect();
        assert_eq!(d.mask.len(), n);
        let out = symmetrize(&d, 0, 0.0, true).unwrap();
        assert_eq!(out.mask.len(), out.positions.len());
        assert_eq!(out.color.len(), out.positions.len());
        assert_eq!(out.groups.len(), out.triangles.len());
        let m = Mesh::from_data(out);
        // Masked on both sides now, and the red top on both sides.
        for (i, p) in m.positions.iter().enumerate() {
            if p.x.abs() > 0.35 {
                assert_eq!(m.mask[i], 1.0, "{p}");
            }
            if p.y > 0.05 {
                assert_eq!(m.color[i], Vec3::X, "{p}");
            }
        }
        assert_eq!(m.groups().iter().copied().max(), Some(1));
    }

    #[test]
    fn symmetrize_needs_something_on_the_kept_side() {
        let d = primitives::icosphere(2, 1.0).to_data();
        assert!(symmetrize(&d, 0, 2.0, true).is_none());
        assert!(symmetrize(&d, 0, -2.0, false).is_none());
        // A plane past the whole mesh on the other side keeps it all, mirrored.
        let m = Mesh::from_data(symmetrize(&d, 0, -2.0, true).unwrap());
        assert_eq!(m.triangle_count(), 2 * d.triangles.len());
    }
}

#[cfg(test)]
mod hole_tests {
    use super::*;
    use crate::{MeshData, primitives};

    fn open_edges(tris: &[[u32; 3]]) -> usize {
        let mut count: HashMap<(u32, u32), u32> = HashMap::new();
        for t in tris {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                *count.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        count.values().filter(|&&c| c == 1).count()
    }

    fn signed_volume(d: &MeshData) -> f32 {
        d.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| d.positions[v as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn closes_two_holes_watertight_facing_out() {
        // A sphere with two caps cut off: two holes.
        let mut d = primitives::icosphere(4, 1.0).to_data();
        let keep: Vec<bool> =
            d.triangles.iter().map(|t| t.iter().all(|&v| d.positions[v as usize].y.abs() < 0.8)).collect();
        let mut k = keep.iter();
        d.triangles.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        d.groups.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        if !d.hidden.is_empty() {
            d.hidden.retain(|_| *k.next().unwrap());
        }
        assert_eq!(boundary_loops(&d.triangles).len(), 2);
        let before_tris = d.triangles.clone();
        let before_pos = d.positions.clone();
        let closed = close_holes(&mut d);
        assert_eq!(closed, 2);
        assert_eq!(open_edges(&d.triangles), 0, "still open");
        assert!(signed_volume(&d) > 3.0, "patches face out (volume {})", signed_volume(&d));
        // Existing geometry untouched; attributes stay in step.
        assert_eq!(&d.triangles[..before_tris.len()], &before_tris[..]);
        assert_eq!(&d.positions[..before_pos.len()], &before_pos[..]);
        assert_eq!(d.groups.len(), d.triangles.len());
        assert_eq!(d.mask.len(), d.positions.len());
        // The patch stays inside the cut: no new vertex pokes far out.
        assert!(d.positions[before_pos.len()..].iter().all(|p| p.y.abs() <= 0.8 + 1e-3 && p.length() < 1.0));
        // A closed mesh has nothing to close.
        let mut whole = primitives::icosphere(3, 1.0).to_data();
        assert_eq!(close_holes(&mut whole), 0);
    }

    /// A tetrahedron with outward-facing triangles.
    fn tetra(p: [Vec3; 4], ids: [u32; 4]) -> Vec<[u32; 3]> {
        let c = (p[0] + p[1] + p[2] + p[3]) / 4.0;
        [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]]
            .into_iter()
            .map(|[a, b, d]| {
                let n = (p[b] - p[a]).cross(p[d] - p[a]);
                if n.dot(p[a] - c) > 0.0 { [ids[a], ids[b], ids[d]] } else { [ids[a], ids[d], ids[b]] }
            })
            .collect()
    }

    #[test]
    fn two_solids_sharing_an_edge_are_split_apart() {
        // Two tetrahedra touching along the edge 0–1, on opposite sides.
        let positions = vec![
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
            Vec3::new(-1.0, -1.0, 0.0),
        ];
        let p = |i: [u32; 4]| i.map(|k| positions[k as usize]);
        let mut triangles = tetra(p([0, 1, 2, 3]), [0, 1, 2, 3]);
        triangles.extend(tetra(p([0, 1, 4, 5]), [0, 1, 4, 5]));
        let mut positions = positions;
        let before = check_manifold(&Mesh::new(positions.clone(), triangles.clone()));
        assert_eq!(before.non_manifold_edge_count, 1);
        let added = split_pinched_edges(&mut positions, &mut triangles);
        assert_eq!(added, 2, "each end of the shared edge gets a copy");
        let after = check_manifold(&Mesh::new(positions.clone(), triangles.clone()));
        assert!(after.is_manifold, "{after:?}");
        // Each tetrahedron is now its own closed piece: they share no point.
        let points = |t: &[[u32; 3]]| t.iter().flatten().copied().collect::<std::collections::HashSet<u32>>();
        assert!(points(&triangles[..4]).is_disjoint(&points(&triangles[4..])));
        // Nothing to do on a clean mesh.
        let mut cube = crate::primitives::cube(2, 1.0);
        assert_eq!(split_pinched_edges(&mut cube.positions, &mut cube.triangles), 0);
    }

    #[test]
    fn specks_are_dropped_but_never_everything() {
        let big = crate::primitives::icosphere(2, 1.0);
        let small = crate::primitives::icosphere(1, 0.01);
        let mut positions = big.positions.clone();
        let mut triangles = big.triangles.clone();
        let n = positions.len() as u32;
        positions.extend(small.positions.iter().map(|p| *p + Vec3::X * 3.0));
        triangles.extend(small.triangles.iter().map(|t| t.map(|i| i + n)));
        assert_eq!(drop_specks(&mut positions, &mut triangles, 0.05), 1);
        assert_eq!((positions.len(), triangles.len()), (big.positions.len(), big.triangles.len()));
        assert!(triangles.iter().flatten().all(|&v| (v as usize) < positions.len()));
        // A lone small piece is kept.
        let mut p = small.positions.clone();
        let mut t = small.triangles.clone();
        assert_eq!(drop_specks(&mut p, &mut t, 0.05), 0);
    }

    #[test]
    fn subdivide_where_splits_only_the_chosen_part_and_stays_closed() {
        let sphere = crate::primitives::icosphere(2, 1.0);
        let data = sphere.to_data();
        // The top cap.
        let chosen: Vec<bool> =
            data.triangles.iter().map(|t| t.iter().all(|&v| data.positions[v as usize].y > 0.6)).collect();
        let picked = chosen.iter().filter(|&&c| c).count();
        for smooth in [false, true] {
            let out = subdivide_where(&data, &chosen, smooth).unwrap();
            let m = Mesh::from_data(out.clone());
            assert!(check_manifold(&m).is_manifold, "closed, no cracks ({smooth})");
            // At least the chosen ones became four each; the bottom is untouched.
            assert!(out.triangles.len() >= data.triangles.len() + 3 * picked);
            let low = |d: &crate::MeshData| {
                d.triangles.iter().filter(|t| t.iter().all(|&v| d.positions[v as usize].y < -0.2)).count()
            };
            assert_eq!(low(&out), low(&data), "the far side isn't split");
            // Old points never move.
            assert_eq!(&out.positions[..data.positions.len()], &data.positions[..]);
            let r = |d: &crate::MeshData, from: usize| {
                d.positions[from..].iter().map(|p| p.length()).fold(f32::MAX, f32::min)
            };
            if smooth {
                // The butterfly curves new points out towards the sphere.
                let flat = subdivide_where(&data, &chosen, false).unwrap();
                assert!(r(&out, data.positions.len()) > r(&flat, data.positions.len()));
            }
            assert_eq!(out.groups.len(), out.triangles.len());
            assert_eq!(out.mask.len(), out.positions.len());
        }
        assert!(subdivide_where(&data, &vec![false; data.triangles.len()], true).is_none());
    }
}
