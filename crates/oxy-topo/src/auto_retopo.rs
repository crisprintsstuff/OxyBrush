//! Field-aligned automatic retopology (Phase 1 of RFC 0044).
//!
//! Implements the local orientation- and position-field pipeline from
//! Jakob et al., *Instant Field-Aligned Meshes* (SIGGRAPH Asia 2015),
//! from the published paper only. Pure Rust; no Instant Meshes / Geogram /
//! Autoremesher C++ source was used.

use glam::Vec3;
use oxy_mesh::{Mesh, MeshData, WHITE};
use std::collections::{HashMap, HashSet};

/// 4-RoSy / 4-PoSy for quad-dominant output.
const SO: usize = 4;
/// Position translations use `sp/2` axes; for quads `sp = so = 4`.
const SP_AXES: usize = SO / 2;

/// Options for [`auto_retopo`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoRetopoOptions {
    /// Approximate number of faces in the result (sets the lattice spacing).
    pub target_faces: u32,
    /// Dihedral angle (degrees) above which edges keep a face normal instead
    /// of a smoothed vertex normal, so creases stay sharp.
    pub sharp_angle_deg: f32,
    /// Use extrinsic (embedding-space) smoothness. Default on.
    pub extrinsic: bool,
    /// Gauss–Seidel passes per hierarchy level.
    pub iterations_per_level: u32,
}

impl Default for AutoRetopoOptions {
    fn default() -> Self {
        Self { target_faces: 5_000, sharp_angle_deg: 45.0, extrinsic: true, iterations_per_level: 6 }
    }
}

/// Why [`auto_retopo`] refused the mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoRetopoError {
    Empty,
    TooFewVertices,
    Degenerate,
}

impl std::fmt::Display for AutoRetopoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoRetopoError::Empty => write!(f, "the piece has no triangles"),
            AutoRetopoError::TooFewVertices => write!(f, "need at least 4 vertices"),
            AutoRetopoError::Degenerate => write!(f, "could not build a retopo mesh from the fields"),
        }
    }
}

/// Rebuild `mesh` as a field-aligned, quad-dominant surface.
pub fn auto_retopo(mesh: &Mesh, opts: AutoRetopoOptions) -> Result<Mesh, AutoRetopoError> {
    if mesh.triangle_count() == 0 {
        return Err(AutoRetopoError::Empty);
    }
    if mesh.vertex_count() < 4 {
        return Err(AutoRetopoError::TooFewVertices);
    }

    let mut graph = Graph::from_mesh(mesh, opts.sharp_angle_deg);
    let area = surface_area(mesh).max(1e-8);
    let target = opts.target_faces.max(4) as f32;
    // ρ ≈ mean edge length of a regular quad mesh covering `area`.
    let rho = (area / target).sqrt().max(1e-6);

    let hierarchy = build_hierarchy(&graph);
    let mut o = random_tangents(&graph);
    let mut p: Vec<Vec3> = graph.positions.clone();

    for level in (0..hierarchy.len()).rev() {
        let g = &hierarchy[level];
        if level + 1 < hierarchy.len() {
            // Prolong from coarser: copy parent field onto children via parent index.
            let parent = &hierarchy[level + 1];
            o = prolong_orientation(g, parent, &o);
            p = prolong_positions(g, parent, &p);
        } else {
            o = random_tangents(g);
            p = g.positions.clone();
        }
        for _ in 0..opts.iterations_per_level.max(1) {
            smooth_orientation(g, &mut o, opts.extrinsic);
            smooth_position(g, &o, &mut p, rho, opts.extrinsic);
        }
        if level == 0 {
            graph = g.clone();
        }
    }

    // Final fine-level passes on the original graph.
    for _ in 0..opts.iterations_per_level.max(1) {
        smooth_orientation(&graph, &mut o, opts.extrinsic);
        smooth_position(&graph, &o, &mut p, rho, opts.extrinsic);
    }

    let data = extract_mesh(&graph, &o, &p, rho)?;
    if data.triangles.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }
    Ok(Mesh::from_data(data))
}

#[derive(Clone)]
struct Graph {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    /// Adjacency lists (undirected).
    neighbors: Vec<Vec<u32>>,
    /// Dual area weights (uniform 1 for Phase 1 hierarchy).
    area: Vec<f32>,
    /// For each vertex, index of its parent on the next coarser level (`None` at coarsest).
    parent: Vec<Option<u32>>,
}

impl Graph {
    fn from_mesh(mesh: &Mesh, sharp_angle_deg: f32) -> Self {
        let n = mesh.vertex_count();
        let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); n];
        let mut face_normals: Vec<Vec3> = Vec::with_capacity(mesh.triangle_count());
        let cos_sharp = (sharp_angle_deg.to_radians()).cos();

        for t in &mesh.triangles {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let fnml = (mesh.positions[b] - mesh.positions[a])
                .cross(mesh.positions[c] - mesh.positions[a])
                .normalize_or_zero();
            face_normals.push(fnml);
            neighbors[a].insert(t[1]);
            neighbors[a].insert(t[2]);
            neighbors[b].insert(t[0]);
            neighbors[b].insert(t[2]);
            neighbors[c].insert(t[0]);
            neighbors[c].insert(t[1]);
        }

        // Edge → incident face normals for crease detection.
        let mut edge_faces: HashMap<[u32; 2], Vec<Vec3>> = HashMap::new();
        for (ti, t) in mesh.triangles.iter().enumerate() {
            let nml = face_normals[ti];
            for e in [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]] {
                let key = if e[0] < e[1] { e } else { [e[1], e[0]] };
                edge_faces.entry(key).or_default().push(nml);
            }
        }
        let mut crease_vert = vec![false; n];
        for (key, faces) in &edge_faces {
            if faces.len() == 2 && faces[0].dot(faces[1]) < cos_sharp {
                crease_vert[key[0] as usize] = true;
                crease_vert[key[1] as usize] = true;
            }
        }

        let mut normals = mesh.normals.clone();
        if normals.len() != n {
            normals = vec![Vec3::Y; n];
            for (ti, t) in mesh.triangles.iter().enumerate() {
                for &v in t {
                    normals[v as usize] += face_normals[ti];
                }
            }
            for nml in &mut normals {
                *nml = nml.normalize_or_zero();
                if *nml == Vec3::ZERO {
                    *nml = Vec3::Y;
                }
            }
        }
        // Crease verts keep a neighboring face normal (paper §3.5).
        for (ti, t) in mesh.triangles.iter().enumerate() {
            for &v in t {
                if crease_vert[v as usize] {
                    normals[v as usize] = face_normals[ti];
                }
            }
        }

        let neighbors: Vec<Vec<u32>> = neighbors.into_iter().map(|s| s.into_iter().collect()).collect();
        Self { positions: mesh.positions.clone(), normals, neighbors, area: vec![1.0; n], parent: vec![None; n] }
    }

    fn len(&self) -> usize {
        self.positions.len()
    }
}

fn surface_area(mesh: &Mesh) -> f32 {
    mesh.triangles
        .iter()
        .map(|t| {
            let (a, b, c) =
                (mesh.positions[t[0] as usize], mesh.positions[t[1] as usize], mesh.positions[t[2] as usize]);
            0.5 * (b - a).cross(c - a).length()
        })
        .sum()
}

fn tangent_basis(n: Vec3) -> (Vec3, Vec3) {
    let n = n.normalize_or_zero();
    let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let a = n.cross(helper).normalize_or_zero();
    let b = n.cross(a).normalize_or_zero();
    (a, b)
}

fn rot_about(n: Vec3, theta: f32, v: Vec3) -> Vec3 {
    // Rodrigues.
    let n = n.normalize_or_zero();
    let (s, c) = theta.sin_cos();
    v * c + n.cross(v) * s + n * n.dot(v) * (1.0 - c)
}

fn rosy(o: Vec3, n: Vec3, k: usize) -> Vec3 {
    rot_about(n, k as f32 * std::f32::consts::TAU / SO as f32, o)
}

fn project_tangent(v: Vec3, n: Vec3) -> Vec3 {
    let n = n.normalize_or_zero();
    (v - n * v.dot(n)).normalize_or_zero()
}

fn random_tangents(g: &Graph) -> Vec<Vec3> {
    // Deterministic pseudo-random from vertex index (no RNG dependency).
    g.normals
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            let (a, b) = tangent_basis(n);
            let t = (i as f32 * 2.399963) % std::f32::consts::TAU;
            project_tangent(a * t.cos() + b * t.sin(), n)
        })
        .collect()
}

fn smooth_orientation(g: &Graph, o: &mut [Vec3], extrinsic: bool) {
    let n = g.len();
    let order: Vec<usize> = (0..n).collect();
    // Mild shuffle via deterministic swap based on iteration-free hash of index.
    let mut order = order;
    for i in 0..n {
        let j = i.wrapping_mul(2654435761) % n;
        order.swap(i, j);
    }
    for &i in &order {
        let ni = g.normals[i];
        let mut acc = Vec3::ZERO;
        let mut wsum = 0.0f32;
        for &j in &g.neighbors[i] {
            let j = j as usize;
            let nj = g.normals[j];
            let w = 1.0f32;
            if extrinsic {
                // Search (k,l) minimizing angle between rotated reps.
                let mut best = (f32::MAX, 0usize, 0usize);
                for k in 0..SO {
                    for l in 0..SO {
                        let a = rosy(o[i], ni, k);
                        let b = rosy(o[j], nj, l);
                        let d = a.angle_between(b);
                        if d < best.0 {
                            best = (d, k, l);
                        }
                    }
                }
                acc += rosy(o[j], nj, best.2) * w;
            } else {
                // Intrinsic: rotate oj into i's tangent plane.
                let axis = nj.cross(ni);
                let ang = nj.angle_between(ni);
                let oj_i = if axis.length_squared() > 1e-10 { rot_about(axis.normalize(), ang, o[j]) } else { o[j] };
                let mut best_k = 0usize;
                let mut best_d = f32::MAX;
                for k in 0..SO {
                    let d = o[i].angle_between(rosy(oj_i, ni, k));
                    if d < best_d {
                        best_d = d;
                        best_k = k;
                    }
                }
                acc += rosy(oj_i, ni, best_k) * w;
            }
            wsum += w;
        }
        if wsum > 0.0 && acc.length_squared() > 1e-12 {
            o[i] = project_tangent(acc / wsum, ni);
            if o[i] == Vec3::ZERO {
                o[i] = project_tangent(tangent_basis(ni).0, ni);
            }
        }
    }
}

/// Integer translation of `p` along the position lattice of (n, o).
fn translate_pos(p: Vec3, n: Vec3, o: Vec3, t: [i32; SP_AXES], rho: f32) -> Vec3 {
    let mut out = p;
    for (k, &tk) in t.iter().enumerate() {
        if tk != 0 {
            out += rosy(o, n, k) * (rho * tk as f32);
        }
    }
    out
}

fn round_to_lattice(n: Vec3, o: Vec3, p: Vec3, v: Vec3, rho: f32) -> Vec3 {
    // Find t minimizing || translate(p,t) - v || among small integers, then
    // project onto the tangent plane at v (paper Eq. 7).
    let n = n.normalize_or_zero();
    let mut best = v + (p - v - n * (p - v).dot(n));
    let mut best_d = best.distance_squared(v);
    for t0 in -2..=2 {
        for t1 in -2..=2 {
            let q = translate_pos(p, n, o, [t0, t1], rho);
            let q_plane = v + (q - v - n * (q - v).dot(n));
            let d = q_plane.distance_squared(v);
            if d < best_d {
                best_d = d;
                best = q_plane;
            }
        }
    }
    best
}

fn smooth_position(g: &Graph, o: &[Vec3], p: &mut [Vec3], rho: f32, extrinsic: bool) {
    let n = g.len();
    let mut order: Vec<usize> = (0..n).collect();
    for i in 0..n {
        let j = i.wrapping_mul(40503) % n;
        order.swap(i, j);
    }
    for &i in &order {
        let ni = g.normals[i];
        let oi = o[i];
        let mut acc = Vec3::ZERO;
        let mut wsum = 0.0f32;
        for &j in &g.neighbors[i] {
            let j = j as usize;
            let nj = g.normals[j];
            let oj = o[j];
            let w = 1.0f32;
            // Midpoint between tangent planes (approx q_ij).
            let q = {
                let mid = (g.positions[i] + g.positions[j]) * 0.5;
                let n_avg = (ni + nj).normalize_or_zero();
                if n_avg == Vec3::ZERO {
                    mid
                } else {
                    mid - n_avg * ((mid - g.positions[i]).dot(ni) + (mid - g.positions[j]).dot(nj)) * 0.5
                }
            };
            // Search small integer jumps near q.
            let mut best_pair = ([0i32; SP_AXES], [0i32; SP_AXES]);
            let mut best_d = f32::MAX;
            for t0 in -1..=1i32 {
                for t1 in -1..=1 {
                    for m0 in -1..=1 {
                        for m1 in -1..=1 {
                            let ti = [t0, t1];
                            let tj = [m0, m1];
                            let pi_t = translate_pos(p[i], ni, oi, ti, rho);
                            let pj_t = translate_pos(p[j], nj, oj, tj, rho);
                            let d = if extrinsic {
                                pi_t.distance_squared(pj_t)
                            } else {
                                // Pull pj into i's plane then compare.
                                let pj_i = q + project_tangent(pj_t - q, ni);
                                pi_t.distance_squared(pj_i)
                            };
                            // Prefer pairs close to q as well.
                            let near = pi_t.distance_squared(q) + pj_t.distance_squared(q);
                            let score = d + 0.05 * near;
                            if score < best_d {
                                best_d = score;
                                best_pair = (ti, tj);
                            }
                        }
                    }
                }
            }
            let contrib = translate_pos(p[j], nj, oj, best_pair.1, rho);
            acc += contrib * w;
            wsum += w;
            let _ = best_pair.0; // tij used during extraction via recompute
        }
        if wsum > 0.0 {
            let avg = acc / wsum;
            p[i] = round_to_lattice(ni, oi, avg, g.positions[i], rho);
        }
    }
}

/// One hierarchy level: collapse high-scoring adjacent pairs (~halve verts).
fn coarsen(g: &Graph) -> Graph {
    let n = g.len();
    let mut scores: Vec<(f32, u32, u32)> = Vec::new();
    for i in 0..n {
        for &j in &g.neighbors[i] {
            let j = j as usize;
            if (i as u32) < j as u32 {
                let s = g.normals[i].dot(g.normals[j]).max(0.0)
                    * (g.area[i] / g.area[j]).min(g.area[j] / g.area[i]).clamp(0.0, 1.0);
                scores.push((s, i as u32, j as u32));
            }
        }
    }
    scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut parent_of = vec![None; n];
    let mut used = vec![false; n];
    let mut new_positions = Vec::new();
    let mut new_normals = Vec::new();
    let mut new_area = Vec::new();
    let mut fine_to_coarse = vec![u32::MAX; n];

    for &(_, i, j) in &scores {
        let (i, j) = (i as usize, j as usize);
        if used[i] || used[j] {
            continue;
        }
        used[i] = true;
        used[j] = true;
        let ai = g.area[i];
        let aj = g.area[j];
        let a = ai + aj;
        let pos = (g.positions[i] * ai + g.positions[j] * aj) / a.max(1e-8);
        let nml = (g.normals[i] * ai + g.normals[j] * aj).normalize_or_zero();
        let idx = new_positions.len() as u32;
        new_positions.push(pos);
        new_normals.push(if nml == Vec3::ZERO { g.normals[i] } else { nml });
        new_area.push(a);
        fine_to_coarse[i] = idx;
        fine_to_coarse[j] = idx;
        parent_of[i] = Some(idx);
        parent_of[j] = Some(idx);
    }
    for i in 0..n {
        if !used[i] {
            let idx = new_positions.len() as u32;
            new_positions.push(g.positions[i]);
            new_normals.push(g.normals[i]);
            new_area.push(g.area[i]);
            fine_to_coarse[i] = idx;
            parent_of[i] = Some(idx);
        }
    }

    let cn = new_positions.len();
    let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); cn];
    for i in 0..n {
        let ci = fine_to_coarse[i];
        for &j in &g.neighbors[i] {
            let cj = fine_to_coarse[j as usize];
            if ci != cj {
                neighbors[ci as usize].insert(cj);
            }
        }
    }
    Graph {
        positions: new_positions,
        normals: new_normals,
        neighbors: neighbors.into_iter().map(|s| s.into_iter().collect()).collect(),
        area: new_area,
        parent: vec![None; cn],
    }
}

fn build_hierarchy(fine: &Graph) -> Vec<Graph> {
    let mut levels = vec![fine.clone()];
    // Stop when small enough or no further collapse.
    while levels.last().unwrap().len() > 32 {
        let next = coarsen(levels.last().unwrap());
        if next.len() >= levels.last().unwrap().len() {
            break;
        }
        // Record parent links on the finer level for prolongation.
        let fine_idx = levels.len() - 1;
        // Rebuild parent on fine from coarsen's fine_to_coarse — coarsen doesn't return map.
        // Re-run a lightweight parent assignment:
        let parent_map = parent_map_for_coarsen(&levels[fine_idx], &next);
        levels[fine_idx].parent = parent_map;
        levels.push(next);
        if levels.len() > 16 {
            break;
        }
    }
    levels
}

fn parent_map_for_coarsen(fine: &Graph, coarse: &Graph) -> Vec<Option<u32>> {
    // Nearest coarse vertex by position (stable enough for prolongation).
    fine.positions
        .iter()
        .map(|&p| {
            let mut best = 0u32;
            let mut best_d = f32::MAX;
            for (ci, &cp) in coarse.positions.iter().enumerate() {
                let d = p.distance_squared(cp);
                if d < best_d {
                    best_d = d;
                    best = ci as u32;
                }
            }
            Some(best)
        })
        .collect()
}

fn prolong_orientation(fine: &Graph, _coarse: &Graph, coarse_o: &[Vec3]) -> Vec<Vec3> {
    fine.parent
        .iter()
        .enumerate()
        .map(|(i, &par)| {
            let c = par.unwrap_or(0) as usize;
            let o = coarse_o.get(c).copied().unwrap_or(Vec3::X);
            project_tangent(o, fine.normals[i])
        })
        .collect()
}

fn prolong_positions(fine: &Graph, _coarse: &Graph, coarse_p: &[Vec3]) -> Vec<Vec3> {
    fine.parent
        .iter()
        .enumerate()
        .map(|(i, &par)| {
            let c = par.unwrap_or(0) as usize;
            coarse_p.get(c).copied().unwrap_or(fine.positions[i])
        })
        .collect()
}

/// Recompute integer jump between i and j for extraction (extrinsic unit search).
fn integer_jump(g: &Graph, o: &[Vec3], p: &[Vec3], rho: f32, i: usize, j: usize) -> [i32; SP_AXES] {
    let ni = g.normals[i];
    let nj = g.normals[j];
    let oi = o[i];
    let oj = o[j];
    let mut best_t = [0i32; SP_AXES];
    let mut best_d = f32::MAX;
    for t0 in -2..=2 {
        for t1 in -2..=2 {
            for m0 in -2..=2 {
                for m1 in -2..=2 {
                    let pi_t = translate_pos(p[i], ni, oi, [t0, t1], rho);
                    let pj_t = translate_pos(p[j], nj, oj, [m0, m1], rho);
                    let d = pi_t.distance_squared(pj_t);
                    if d < best_d {
                        best_d = d;
                        // Store tij − tji style as the jump from i toward j.
                        best_t = [t0 - m0, t1 - m1];
                    }
                }
            }
        }
    }
    best_t
}

fn is_unit_jump(t: [i32; SP_AXES]) -> bool {
    let abs: i32 = t[0].abs() + t[1].abs();
    abs == 1
}

fn is_zero_jump(t: [i32; SP_AXES]) -> bool {
    t[0] == 0 && t[1] == 0
}

fn extract_mesh(g: &Graph, o: &[Vec3], p: &[Vec3], rho: f32) -> Result<MeshData, AutoRetopoError> {
    let n = g.len();
    // Union-find over input verts: merge on zero jumps (short distance first).
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let find = |parent: &mut [u32], mut x: u32| {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    };
    let unite = |parent: &mut [u32], a: u32, b: u32| {
        let pa = find(parent, a);
        let pb = find(parent, b);
        if pa != pb {
            parent[pa as usize] = pb;
        }
    };

    let mut zero_edges: Vec<(f32, u32, u32)> = Vec::new();
    let mut unit_edges: Vec<(u32, u32)> = Vec::new();
    for i in 0..n {
        for &j in &g.neighbors[i] {
            if (i as u32) < j {
                let t = integer_jump(g, o, p, rho, i, j as usize);
                if is_zero_jump(t) {
                    zero_edges.push((p[i].distance(p[j as usize]), i as u32, j));
                } else if is_unit_jump(t) {
                    unit_edges.push((i as u32, j));
                }
            }
        }
    }
    zero_edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Collapse zero-jump clusters unless already linked by a unit edge (paper heuristic).
    let unit_set: HashSet<(u32, u32)> = unit_edges.iter().map(|&(a, b)| if a < b { (a, b) } else { (b, a) }).collect();
    for &(_, a, b) in &zero_edges {
        let key = if a < b { (a, b) } else { (b, a) };
        if unit_set.contains(&key) {
            continue;
        }
        unite(&mut parent, a, b);
    }

    // Cluster → output vertex.
    let mut cluster_members: HashMap<u32, Vec<u32>> = HashMap::new();
    for i in 0..n as u32 {
        let r = find(&mut parent, i);
        cluster_members.entry(r).or_default().push(i);
    }
    let avg_count = (cluster_members.values().map(|v| v.len()).sum::<usize>() as f32
        / cluster_members.len().max(1) as f32)
        .max(1.0);
    let min_count = (avg_count / 10.0).max(1.0) as usize;

    let mut out_pos = Vec::new();
    let mut out_nml = Vec::new();
    let mut root_to_out: HashMap<u32, u32> = HashMap::new();
    for (root, members) in &cluster_members {
        if members.len() < min_count {
            continue;
        }
        let mut pos = Vec3::ZERO;
        let mut nml = Vec3::ZERO;
        let mut wsum = 0.0f32;
        let beta = rho / 3.0;
        let beta2 = (beta * beta).max(1e-12);
        for &mi in members {
            let i = mi as usize;
            let w = (-p[i].distance_squared(g.positions[i]) / beta2).exp();
            pos += p[i] * w;
            nml += g.normals[i] * w;
            wsum += w;
        }
        if wsum < 1e-8 {
            continue;
        }
        let idx = out_pos.len() as u32;
        out_pos.push(pos / wsum);
        out_nml.push(nml.normalize_or_zero());
        root_to_out.insert(*root, idx);
    }
    if out_pos.len() < 4 {
        return Err(AutoRetopoError::Degenerate);
    }

    // Output edges from unit jumps between different clusters.
    let mut out_adj: Vec<HashSet<u32>> = vec![HashSet::new(); out_pos.len()];
    for &(a, b) in &unit_edges {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra == rb {
            continue;
        }
        let (Some(&oa), Some(&ob)) = (root_to_out.get(&ra), root_to_out.get(&rb)) else {
            continue;
        };
        if oa != ob {
            out_adj[oa as usize].insert(ob);
            out_adj[ob as usize].insert(oa);
        }
    }

    // Face walk: for each oriented edge, take the sharpest left turn in the tangent plane.
    let mut triangles = Vec::new();
    let mut used_oriented: HashSet<(u32, u32)> = HashSet::new();
    for start in 0..out_pos.len() as u32 {
        let neighbors: Vec<u32> = out_adj[start as usize].iter().copied().collect();
        for next in neighbors {
            if used_oriented.contains(&(start, next)) {
                continue;
            }
            let mut face: Vec<u32> = vec![start];
            let mut prev = start;
            let mut cur = next;
            used_oriented.insert((prev, cur));
            let mut guard = 0;
            while cur != start && guard < 16 {
                guard += 1;
                face.push(cur);
                let nml = out_nml[cur as usize];
                let incoming = (out_pos[cur as usize] - out_pos[prev as usize]).normalize_or_zero();
                let mut best_next = None;
                let mut best_score = f32::MIN;
                for &cand in &out_adj[cur as usize] {
                    if cand == prev {
                        continue;
                    }
                    if used_oriented.contains(&(cur, cand)) && cand != start {
                        continue;
                    }
                    let outgoing = (out_pos[cand as usize] - out_pos[cur as usize]).normalize_or_zero();
                    let cross = incoming.cross(outgoing).dot(nml);
                    let dot = incoming.dot(outgoing).clamp(-1.0, 1.0);
                    let score = cross.atan2(dot);
                    if score > best_score {
                        best_score = score;
                        best_next = Some(cand);
                    }
                }
                let Some(nxt) = best_next else { break };
                prev = cur;
                cur = nxt;
                used_oriented.insert((prev, cur));
            }
            if cur == start && face.len() >= 3 && face.len() <= 6 {
                for k in 1..face.len() - 1 {
                    triangles.push([face[0], face[k], face[k + 1]]);
                }
            }
        }
    }

    if triangles.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }

    let nv = out_pos.len();
    let nt = triangles.len();
    Ok(MeshData {
        positions: out_pos,
        triangles,
        mask: vec![0.0; nv],
        color: vec![WHITE; nv],
        groups: vec![0; nt],
        hidden: vec![false; nt],
        uv: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn auto_retopo_sphere_produces_mesh() {
        let mesh = primitives::icosphere(3, 1.0);
        let out =
            auto_retopo(&mesh, AutoRetopoOptions { target_faces: 400, iterations_per_level: 4, ..Default::default() })
                .expect("retopo sphere");
        assert!(out.triangle_count() > 10, "got {} tris", out.triangle_count());
        assert!(out.vertex_count() > 10);
        assert!(out.triangle_count() < mesh.triangle_count());
    }

    #[test]
    fn auto_retopo_empty_errors() {
        let mesh = Mesh::from_data(MeshData {
            positions: Vec::new(),
            triangles: Vec::new(),
            mask: Vec::new(),
            color: Vec::new(),
            groups: Vec::new(),
            hidden: Vec::new(),
            uv: Vec::new(),
        });
        assert!(matches!(auto_retopo(&mesh, AutoRetopoOptions::default()), Err(AutoRetopoError::Empty)));
    }
}
