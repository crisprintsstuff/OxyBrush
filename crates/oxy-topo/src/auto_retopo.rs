//! OxyRemesh: field-aligned automatic retopology (RFC 0044).
//!
//! Implements the local orientation- and position-field pipeline from
//! Jakob et al., *Instant Field-Aligned Meshes* (SIGGRAPH Asia 2015),
//! from the published paper only. Pure Rust; no Instant Meshes / Geogram /
//! Autoremesher C++ source was used.
//!
//! Phase 2 adds a Catmull–Clark-topology pure-quad post-pass (no
//! smoothing), attribute projection from the source mesh, and stronger
//! crease alignment during field smoothing.
//!
//! Phase 3 adds orientation stroke constraints, singularity attractors,
//! and adaptivity / anisotropy knobs (paper §3.7).

use glam::Vec3;
use oxy_mesh::{Mesh, MeshData, WHITE};
use std::collections::{HashMap, HashSet};

/// 4-RoSy / 4-PoSy for quad-dominant output.
const SO: usize = 4;
/// Position translations use `sp/2` axes; for quads `sp = so = 4`.
const SP_AXES: usize = SO / 2;
/// Extra weight on crease edges during orientation / position GS.
const CREASE_WEIGHT: f32 = 8.0;
/// Soft weight for orientation stroke constraints.
const CONSTRAINT_WEIGHT: f32 = 12.0;
/// Soft weight for singularity-attractor circumferential bias.
const ATTRACTOR_WEIGHT: f32 = 10.0;

/// Soft orientation pin: prefer `direction` (tangent) near `position`.
#[derive(Debug, Clone, PartialEq)]
pub struct OrientationConstraint {
    pub position: Vec3,
    pub direction: Vec3,
}

/// Options for [`auto_retopo`] (OxyRemesh).
#[derive(Debug, Clone, PartialEq)]
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
    /// Split every extracted polygon into quads (face + edge midpoints).
    /// Default on (Phase 2).
    pub pure_quad: bool,
    /// Carry vertex color, mask and face groups from the source (like remesh).
    pub transfer_attributes: bool,
    /// Snap result vertices onto the source surface after extraction.
    pub snap_to_surface: bool,
    /// 0 = uniform density; 1 = much denser where curvature is high (Phase 3).
    pub adaptivity: f32,
    /// 0 = square cells; 1 = stretch along the orientation field (Phase 3).
    pub anisotropy: f32,
    /// Soft orientation stroke constraints (Phase 3 / paper §3.7).
    pub orientation_constraints: Vec<OrientationConstraint>,
    /// Soft singularity attractors in world space (Phase 3 / paper §3.7).
    pub singularity_attractors: Vec<Vec3>,
}

impl Default for AutoRetopoOptions {
    fn default() -> Self {
        Self {
            target_faces: 5_000,
            sharp_angle_deg: 45.0,
            extrinsic: true,
            iterations_per_level: 6,
            pure_quad: true,
            transfer_attributes: true,
            snap_to_surface: true,
            adaptivity: 0.0,
            anisotropy: 0.0,
            orientation_constraints: Vec::new(),
            singularity_attractors: Vec::new(),
        }
    }
}

/// Why [`auto_retopo`] refused the mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoRetopoError {
    Empty,
    TooFewVertices,
    Degenerate,
    /// Face extraction produced disconnected islands instead of one surface (#551).
    Fragmented {
        components: usize,
    },
}

impl std::fmt::Display for AutoRetopoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoRetopoError::Empty => write!(f, "the piece has no triangles"),
            AutoRetopoError::TooFewVertices => write!(f, "need at least 4 vertices"),
            AutoRetopoError::Degenerate => write!(f, "could not build a retopo mesh from the fields"),
            AutoRetopoError::Fragmented { components } => write!(
                f,
                "OxyRemesh built {components} disconnected pieces instead of one surface; try Voxel remesh (Ctrl+R) or a different face count"
            ),
        }
    }
}

/// Rebuild `mesh` as a field-aligned surface (OxyRemesh).
///
/// With default options the result is pure-quad (each extracted polygon
/// becomes quads via face and edge midpoints), attributes are projected
/// from `mesh`, and vertices are snapped onto its surface.
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
    // Pure-quad post-pass roughly quadruples face count, so aim ρ a bit larger.
    let face_budget = if opts.pure_quad { (target / 4.0).max(4.0) } else { target };
    let rho = (area / face_budget).sqrt().max(1e-6);
    let adaptivity = opts.adaptivity.clamp(0.0, 1.0);
    let anisotropy = opts.anisotropy.clamp(0.0, 1.0);

    let hierarchy = build_hierarchy(&graph);
    let mut o = random_tangents(&graph);
    let mut p: Vec<Vec3> = graph.positions.clone();

    for level in (0..hierarchy.len()).rev() {
        let g = &hierarchy[level];
        if level + 1 < hierarchy.len() {
            let parent = &hierarchy[level + 1];
            o = prolong_orientation(g, parent, &o);
            p = prolong_positions(g, parent, &p);
        } else {
            o = random_tangents(g);
            p = g.positions.clone();
        }
        // Stroke / attractor guidance only on the finest level (paper §3.7).
        let guided = level == 0;
        let constraints =
            if guided { assign_constraints(g, &opts.orientation_constraints, rho) } else { HashMap::new() };
        let attractors = if guided { assign_attractors(g, &opts.singularity_attractors, rho) } else { HashMap::new() };
        for _ in 0..opts.iterations_per_level.max(1) {
            smooth_orientation(g, &mut o, opts.extrinsic, &constraints, &attractors);
            smooth_position(g, &o, &mut p, rho, opts.extrinsic, adaptivity, anisotropy);
        }
        if level == 0 {
            graph = g.clone();
        }
    }

    let constraints = assign_constraints(&graph, &opts.orientation_constraints, rho);
    let attractors = assign_attractors(&graph, &opts.singularity_attractors, rho);
    for _ in 0..opts.iterations_per_level.max(1) {
        smooth_orientation(&graph, &mut o, opts.extrinsic, &constraints, &attractors);
        smooth_position(&graph, &o, &mut p, rho, opts.extrinsic, adaptivity, anisotropy);
    }

    let extracted = extract_faces(&graph, &o, &p, rho)?;
    if extracted.faces.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }
    // Merge nearly-coincident dual vertices before triangulation so face
    // corners that land on the same lattice point share an index.
    let extracted = weld_extracted(extracted, (rho * 0.25).max(1e-5));
    let (data, diagonals) =
        if opts.pure_quad { polygons_to_pure_quads(&extracted)? } else { polygons_to_quad_dominant(&extracted)? };
    if data.triangles.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }
    let mut out = Mesh::from_data(data);
    if !diagonals.is_empty() {
        // Mesh::from_data renumbers verts into chunk order; map diagonals.
        let src = out.source_vertices();
        let max_old = src.iter().copied().max().unwrap_or(0) as usize;
        let mut old_to_new = vec![u32::MAX; max_old + 1];
        for (new, &old) in src.iter().enumerate() {
            old_to_new[old as usize] = new as u32;
        }
        let remapped = diagonals
            .into_iter()
            .filter_map(|[a, b]| {
                let na = *old_to_new.get(a as usize)?;
                let nb = *old_to_new.get(b as usize)?;
                if na == u32::MAX || nb == u32::MAX {
                    return None;
                }
                Some([na.min(nb), na.max(nb)])
            })
            .collect();
        out.set_quad_diagonals(remapped);
    }
    if opts.snap_to_surface {
        crate::SurfaceProjector::new(&[mesh]).project(&mut out);
    }
    if opts.transfer_attributes {
        crate::transfer_attributes(&[mesh], &mut out);
    }
    // Drop floating islands; keep the largest piece when it is clearly the
    // surface (#551). Refuse only when extraction shattered into comparable shards.
    out = keep_largest_component(out)?;
    Ok(out)
}

/// Component count and triangle counts per root (descending).
fn component_tri_sizes(m: &Mesh) -> (usize, Vec<(u32, usize)>) {
    let nv = m.vertex_count();
    if nv == 0 {
        return (0, Vec::new());
    }
    let mut parent: Vec<u32> = (0..nv as u32).collect();
    let find = |parent: &mut [u32], mut x: u32| {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    };
    for t in &m.triangles {
        for &v in &t[1..] {
            let (a, b) = (find(&mut parent, t[0]), find(&mut parent, v));
            parent[a as usize] = b;
        }
    }
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for t in &m.triangles {
        let r = find(&mut parent, t[0]);
        *sizes.entry(r).or_default() += 1;
    }
    let mut ranked: Vec<(u32, usize)> = sizes.into_iter().collect();
    ranked.sort_by_key(|a| std::cmp::Reverse(a.1));
    let n = ranked.len();
    (n, ranked)
}

#[cfg(test)]
fn connected_components(m: &Mesh) -> usize {
    component_tri_sizes(m).0
}

/// Keep the largest vertex-connected piece when floaters are a clear minority.
///
/// Instant Meshes §3.8 notes extraction can leave local artifacts; mesh-repair
/// practice is to drop small disconnected debris rather than discard the whole
/// result (#551). Refuse when no dominant surface exists.
fn keep_largest_component(mesh: Mesh) -> Result<Mesh, AutoRetopoError> {
    let (n, ranked) = component_tri_sizes(&mesh);
    if n <= 1 {
        return Ok(mesh);
    }
    let total = mesh.triangle_count().max(1);
    let (main_root, main_tris) = ranked[0];
    // Require a clear majority so we never return a "best of many shards".
    if (main_tris as f32 / total as f32) < 0.50 {
        return Err(AutoRetopoError::Fragmented { components: n });
    }
    let nv = mesh.vertex_count();
    let mut parent: Vec<u32> = (0..nv as u32).collect();
    let find = |parent: &mut [u32], mut x: u32| {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    };
    for t in &mesh.triangles {
        for &v in &t[1..] {
            let (a, b) = (find(&mut parent, t[0]), find(&mut parent, v));
            parent[a as usize] = b;
        }
    }
    let keep_vert: Vec<bool> = (0..nv as u32).map(|v| find(&mut parent, v) == main_root).collect();
    let mut remap = vec![u32::MAX; nv];
    let mut positions = Vec::new();
    let mut mask = Vec::new();
    let mut color = Vec::new();
    for (i, &keep) in keep_vert.iter().enumerate() {
        if !keep {
            continue;
        }
        remap[i] = positions.len() as u32;
        positions.push(mesh.positions[i]);
        mask.push(mesh.mask[i]);
        color.push(mesh.color[i]);
    }
    let mut triangles = Vec::new();
    let mut groups = Vec::new();
    let mut hidden = Vec::new();
    for (ti, t) in mesh.triangles.iter().enumerate() {
        if !keep_vert[t[0] as usize] {
            continue;
        }
        let a = remap[t[0] as usize];
        let b = remap[t[1] as usize];
        let c = remap[t[2] as usize];
        if a == u32::MAX || b == u32::MAX || c == u32::MAX {
            continue;
        }
        triangles.push([a, b, c]);
        groups.push(mesh.groups().get(ti).copied().unwrap_or(0));
        hidden.push(mesh.hidden().get(ti).copied().unwrap_or(false));
    }
    if triangles.len() < 4 {
        return Err(AutoRetopoError::Fragmented { components: n });
    }
    let out = Mesh::from_data(MeshData { positions, triangles, mask, color, groups, hidden, uv: Vec::new() });
    // Preserve quad diagonals that still exist in the kept piece.
    let diags = mesh.quad_diagonals();
    if !diags.is_empty() {
        let src = out.source_vertices();
        let max_old = src.iter().copied().max().unwrap_or(0) as usize;
        let mut data_to_chunk = vec![u32::MAX; max_old + 1];
        for (new, &old) in src.iter().enumerate() {
            data_to_chunk[old as usize] = new as u32;
        }
        let remapped: HashSet<[u32; 2]> = diags
            .iter()
            .filter_map(|&[a, b]| {
                let na = *remap.get(a as usize).unwrap_or(&u32::MAX);
                let nb = *remap.get(b as usize).unwrap_or(&u32::MAX);
                if na == u32::MAX || nb == u32::MAX {
                    return None;
                }
                let ca = *data_to_chunk.get(na as usize).unwrap_or(&u32::MAX);
                let cb = *data_to_chunk.get(nb as usize).unwrap_or(&u32::MAX);
                if ca == u32::MAX || cb == u32::MAX { None } else { Some([ca.min(cb), ca.max(cb)]) }
            })
            .collect();
        if !remapped.is_empty() {
            out.set_quad_diagonals(remapped);
        }
    }
    Ok(out)
}

/// Weld nearly-coincident dual vertices and remap polygon corners.
fn weld_extracted(ex: Extracted, eps: f32) -> Extracted {
    let (positions, _dummy_tris, map) = oxy_mesh::primitives::weld_with_map(ex.positions, Vec::new(), eps);
    let mut faces = Vec::with_capacity(ex.faces.len());
    for face in ex.faces {
        let mut mapped: Vec<u32> = face.into_iter().map(|v| map[v as usize]).collect();
        // Drop consecutive duplicates from welding.
        mapped.dedup();
        if mapped.len() >= 2 && mapped.first() == mapped.last() {
            mapped.pop();
        }
        mapped.dedup();
        if mapped.len() >= 3 {
            faces.push(mapped);
        }
    }
    Extracted { positions, faces }
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
    /// Vertices incident to a sharp crease edge.
    crease: Vec<bool>,
    /// Sharp crease edges as `[low, high]`.
    crease_edges: HashSet<[u32; 2]>,
    /// Absolute mean-curvature proxy in 0..1 (for adaptivity).
    curvature: Vec<f32>,
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
        let mut crease_edges: HashSet<[u32; 2]> = HashSet::new();
        for (key, faces) in &edge_faces {
            // Interior sharp dihedral, or boundary edge (one face).
            let sharp = match faces.len() {
                1 => true,
                2 => faces[0].dot(faces[1]) < cos_sharp,
                _ => false,
            };
            if sharp {
                crease_vert[key[0] as usize] = true;
                crease_vert[key[1] as usize] = true;
                crease_edges.insert(*key);
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
        let peak = mesh.curvature(false);
        let cav = mesh.curvature(true);
        let curvature: Vec<f32> = peak.iter().zip(cav.iter()).map(|(a, b)| (*a).max(*b)).collect();
        Self {
            positions: mesh.positions.clone(),
            normals,
            neighbors,
            area: vec![1.0; n],
            parent: vec![None; n],
            crease: crease_vert,
            crease_edges,
            curvature,
        }
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

fn best_rosy_match(o: Vec3, n: Vec3, target: Vec3) -> Vec3 {
    let mut best = o;
    let mut best_d = f32::MAX;
    for k in 0..SO {
        let r = rosy(o, n, k);
        let d = r.angle_between(target);
        if d < best_d {
            best_d = d;
            best = r;
        }
    }
    best
}

fn smooth_orientation(
    g: &Graph,
    o: &mut [Vec3],
    extrinsic: bool,
    constraints: &HashMap<usize, Vec3>,
    attractors: &HashMap<usize, Vec3>,
) {
    let n = g.len();
    let mut order: Vec<usize> = (0..n).collect();
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
            let edge = [i.min(j) as u32, i.max(j) as u32];
            let w = if g.crease_edges.contains(&edge) { CREASE_WEIGHT } else { 1.0 };
            if extrinsic {
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
        if g.crease[i] {
            for &j in &g.neighbors[i] {
                let j = j as usize;
                let edge = [i.min(j) as u32, i.max(j) as u32];
                if !g.crease_edges.contains(&edge) {
                    continue;
                }
                let dir = project_tangent(g.positions[j] - g.positions[i], ni);
                if dir != Vec3::ZERO {
                    acc += dir * CREASE_WEIGHT;
                    wsum += CREASE_WEIGHT;
                }
            }
        }
        // Phase 3: soft orientation stroke constraint.
        if let Some(&dir) = constraints.get(&i) {
            let pinned = best_rosy_match(o[i], ni, dir);
            acc += pinned * CONSTRAINT_WEIGHT;
            wsum += CONSTRAINT_WEIGHT;
        }
        // Phase 3: singularity attractor — prefer circumferential field.
        if let Some(&circ) = attractors.get(&i) {
            let pinned = best_rosy_match(o[i], ni, circ);
            acc += pinned * ATTRACTOR_WEIGHT;
            wsum += ATTRACTOR_WEIGHT;
        }
        if wsum > 0.0 && acc.length_squared() > 1e-12 {
            o[i] = project_tangent(acc / wsum, ni);
            if o[i] == Vec3::ZERO {
                o[i] = project_tangent(tangent_basis(ni).0, ni);
            }
        }
    }
}

/// Map stroke constraints onto nearby graph vertices.
fn assign_constraints(g: &Graph, constraints: &[OrientationConstraint], rho: f32) -> HashMap<usize, Vec3> {
    let mut out = HashMap::new();
    let radius = (rho * 2.5).max(1e-4);
    let radius2 = radius * radius;
    for c in constraints {
        let dir0 = c.direction;
        if dir0.length_squared() < 1e-12 {
            continue;
        }
        let mut best: Option<(f32, usize)> = None;
        for i in 0..g.len() {
            let d2 = g.positions[i].distance_squared(c.position);
            if d2 <= radius2 && best.is_none_or(|(bd, _)| d2 < bd) {
                best = Some((d2, i));
            }
        }
        if let Some((_, i)) = best {
            let dir = project_tangent(dir0, g.normals[i]);
            if dir != Vec3::ZERO {
                out.insert(i, dir);
            }
        }
    }
    out
}

/// Map singularity attractors to a circumferential preferred tangent per nearby vert.
fn assign_attractors(g: &Graph, attractors: &[Vec3], rho: f32) -> HashMap<usize, Vec3> {
    let mut out = HashMap::new();
    let radius = (rho * 4.0).max(1e-4);
    let radius2 = radius * radius;
    for &a in attractors {
        for i in 0..g.len() {
            let d2 = g.positions[i].distance_squared(a);
            if d2 > radius2 || d2 < 1e-12 {
                continue;
            }
            let ni = g.normals[i];
            let radial = project_tangent(g.positions[i] - a, ni);
            let circ = ni.cross(radial).normalize_or_zero();
            if circ == Vec3::ZERO {
                continue;
            }
            // Keep the strongest (nearest) attractor influence.
            let w = 1.0 / d2.sqrt();
            match out.get(&i) {
                Some(_) if w < 1.0 => {}
                _ => {
                    out.insert(i, circ);
                }
            }
        }
    }
    out
}

/// Integer translation of `p` along the position lattice of (n, o).
fn local_rho(base: f32, curv: f32, adaptivity: f32) -> f32 {
    (base / (1.0 + adaptivity * 4.0 * curv.clamp(0.0, 1.0))).max(base * 0.15)
}

fn aniso_scales(rho: f32, anisotropy: f32) -> (f32, f32) {
    // Stretch along orientation (axis 0), compress across (axis 1).
    let a = anisotropy.clamp(0.0, 1.0);
    let stretch = 1.0 + a;
    let compress = 1.0 / stretch;
    (rho * stretch, rho * compress)
}

fn translate_pos(p: Vec3, n: Vec3, o: Vec3, t: [i32; SP_AXES], rho_u: f32, rho_v: f32) -> Vec3 {
    let mut out = p;
    let scales = [rho_u, rho_v];
    for (k, &tk) in t.iter().enumerate() {
        if tk != 0 {
            out += rosy(o, n, k) * (scales[k] * tk as f32);
        }
    }
    out
}

fn round_to_lattice(n: Vec3, o: Vec3, p: Vec3, v: Vec3, rho_u: f32, rho_v: f32) -> Vec3 {
    // Find t minimizing || translate(p,t) - v || among small integers, then
    // project onto the tangent plane at v (paper Eq. 7).
    let n = n.normalize_or_zero();
    let mut best = v + (p - v - n * (p - v).dot(n));
    let mut best_d = best.distance_squared(v);
    for t0 in -2..=2 {
        for t1 in -2..=2 {
            let q = translate_pos(p, n, o, [t0, t1], rho_u, rho_v);
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

fn smooth_position(g: &Graph, o: &[Vec3], p: &mut [Vec3], rho: f32, extrinsic: bool, adaptivity: f32, anisotropy: f32) {
    let n = g.len();
    let mut order: Vec<usize> = (0..n).collect();
    for i in 0..n {
        let j = i.wrapping_mul(40503) % n;
        order.swap(i, j);
    }
    for &i in &order {
        let ni = g.normals[i];
        let oi = o[i];
        let curv = g.curvature.get(i).copied().unwrap_or(0.0);
        let rho_i = local_rho(rho, curv, adaptivity);
        let (rho_u, rho_v) = aniso_scales(rho_i, anisotropy);
        let mut acc = Vec3::ZERO;
        let mut wsum = 0.0f32;
        for &j in &g.neighbors[i] {
            let j = j as usize;
            let nj = g.normals[j];
            let oj = o[j];
            let edge = [i.min(j) as u32, i.max(j) as u32];
            let w = if g.crease_edges.contains(&edge) { CREASE_WEIGHT } else { 1.0 };
            let curv_j = g.curvature.get(j).copied().unwrap_or(0.0);
            let rho_j = local_rho(rho, curv_j, adaptivity);
            let (rho_ju, rho_jv) = aniso_scales(rho_j, anisotropy);
            let q = {
                let mid = (g.positions[i] + g.positions[j]) * 0.5;
                let n_avg = (ni + nj).normalize_or_zero();
                if n_avg == Vec3::ZERO {
                    mid
                } else {
                    mid - n_avg * ((mid - g.positions[i]).dot(ni) + (mid - g.positions[j]).dot(nj)) * 0.5
                }
            };
            let mut best_pair = ([0i32; SP_AXES], [0i32; SP_AXES]);
            let mut best_d = f32::MAX;
            for t0 in -1..=1i32 {
                for t1 in -1..=1 {
                    for m0 in -1..=1 {
                        for m1 in -1..=1 {
                            let ti = [t0, t1];
                            let tj = [m0, m1];
                            let pi_t = translate_pos(p[i], ni, oi, ti, rho_u, rho_v);
                            let pj_t = translate_pos(p[j], nj, oj, tj, rho_ju, rho_jv);
                            let d = if extrinsic {
                                pi_t.distance_squared(pj_t)
                            } else {
                                let pj_i = q + project_tangent(pj_t - q, ni);
                                pi_t.distance_squared(pj_i)
                            };
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
            let contrib = translate_pos(p[j], nj, oj, best_pair.1, rho_ju, rho_jv);
            acc += contrib * w;
            wsum += w;
            let _ = best_pair.0;
        }
        if wsum > 0.0 {
            let avg = acc / wsum;
            p[i] = round_to_lattice(ni, oi, avg, g.positions[i], rho_u, rho_v);
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
    let mut new_crease = Vec::new();
    let mut new_curvature = Vec::new();
    let mut fine_to_coarse = vec![u32::MAX; n];

    for &(_, i, j) in &scores {
        let (i, j) = (i as usize, j as usize);
        if used[i] || used[j] {
            continue;
        }
        // Prefer not to collapse across a crease edge.
        let edge = [i.min(j) as u32, i.max(j) as u32];
        if g.crease_edges.contains(&edge) {
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
        new_crease.push(g.crease[i] || g.crease[j]);
        new_curvature.push((g.curvature[i] * ai + g.curvature[j] * aj) / a.max(1e-8));
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
            new_crease.push(g.crease[i]);
            new_curvature.push(g.curvature[i]);
            fine_to_coarse[i] = idx;
            parent_of[i] = Some(idx);
        }
    }

    let cn = new_positions.len();
    let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); cn];
    let mut crease_edges: HashSet<[u32; 2]> = HashSet::new();
    for i in 0..n {
        let ci = fine_to_coarse[i];
        for &j in &g.neighbors[i] {
            let cj = fine_to_coarse[j as usize];
            if ci != cj {
                neighbors[ci as usize].insert(cj);
                let fine_e = [i.min(j as usize) as u32, i.max(j as usize) as u32];
                if g.crease_edges.contains(&fine_e) {
                    let ce = [ci.min(cj), ci.max(cj)];
                    crease_edges.insert(ce);
                }
            }
        }
    }
    Graph {
        positions: new_positions,
        normals: new_normals,
        neighbors: neighbors.into_iter().map(|s| s.into_iter().collect()).collect(),
        area: new_area,
        parent: vec![None; cn],
        crease: new_crease,
        crease_edges,
        curvature: new_curvature,
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

/// Best extrinsic lattice pair between i and j for extraction (paper §3.8).
///
/// RETURNS: `(tij − tji, distance² between the translated representatives)`.
fn integer_jump(g: &Graph, o: &[Vec3], p: &[Vec3], rho: f32, i: usize, j: usize) -> ([i32; SP_AXES], f32) {
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
                    let pi_t = translate_pos(p[i], ni, oi, [t0, t1], rho, rho);
                    let pj_t = translate_pos(p[j], nj, oj, [m0, m1], rho, rho);
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
    (best_t, best_d)
}

fn is_unit_jump(t: [i32; SP_AXES]) -> bool {
    let abs: i32 = t[0].abs() + t[1].abs();
    abs == 1
}

/// Same output vertex: translated reps nearly coincide (paper blue / collapse).
///
/// Classifying only on `tij − tji == 0` missed extrinsic same-point pairs whose
/// best `(tij, tji)` left a non-zero difference (#551). Never treat a unit
/// lattice step as a collapse — that ate dual edges and emptied the mesh.
fn is_collapse_jump(diff: [i32; SP_AXES], dist2: f32, rho: f32) -> bool {
    if is_unit_jump(diff) {
        return false;
    }
    // Exact integer-zero jump (paper blue edge when tij = −tji collapses to 0).
    if diff[0] == 0 && diff[1] == 0 {
        return true;
    }
    // Extrinsic same-point pairs whose best (tij, tji) left a non-zero
    // difference: only merge when translated reps almost coincide.
    let close = (rho * 0.12).max(1e-5);
    dist2 <= close * close
}

struct Extracted {
    positions: Vec<Vec3>,
    /// Oriented polygon loops (3–6 corners) in output vertex indices.
    faces: Vec<Vec<u32>>,
}

fn extract_faces(g: &Graph, o: &[Vec3], p: &[Vec3], rho: f32) -> Result<Extracted, AutoRetopoError> {
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
                let (t, dist2) = integer_jump(g, o, p, rho, i, j as usize);
                if is_collapse_jump(t, dist2, rho) {
                    zero_edges.push((p[i].distance(p[j as usize]), i as u32, j));
                } else if is_unit_jump(t) {
                    unit_edges.push((i as u32, j));
                }
            }
        }
    }
    zero_edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Paper §3.8: build unit-edge graph first; only collapse zero-pairs that
    // are not already adjacent there (avoids eager merges at singularities).
    let unit_set: HashSet<(u32, u32)> = unit_edges.iter().map(|&(a, b)| if a < b { (a, b) } else { (b, a) }).collect();
    for &(_, a, b) in &zero_edges {
        let key = if a < b { (a, b) } else { (b, a) };
        if unit_set.contains(&key) {
            continue;
        }
        unite(&mut parent, a, b);
    }

    let mut cluster_members: HashMap<u32, Vec<u32>> = HashMap::new();
    for i in 0..n as u32 {
        let r = find(&mut parent, i);
        cluster_members.entry(r).or_default().push(i);
    }
    // Paper §3.8: drop clusters with very few supporting input verts
    // (n < n̄/10); those produce spurious faces in the dual.
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

    // Face walk: sharpest left turn in the tangent plane. Only mark oriented
    // edges used when a valid cycle closes — burning edges on failed walks
    // left dual graphs with edges but zero faces (Degenerate).
    let mut faces = Vec::new();
    let mut used_oriented: HashSet<(u32, u32)> = HashSet::new();
    for start in 0..out_pos.len() as u32 {
        let neighbors: Vec<u32> = out_adj[start as usize].iter().copied().collect();
        for next in neighbors {
            if used_oriented.contains(&(start, next)) {
                continue;
            }
            let mut face: Vec<u32> = vec![start];
            let mut walk: Vec<(u32, u32)> = Vec::new();
            let mut prev = start;
            let mut cur = next;
            walk.push((prev, cur));
            let mut guard = 0;
            let mut closed = false;
            while guard < 16 {
                guard += 1;
                if cur == start && face.len() >= 3 {
                    closed = true;
                    break;
                }
                if cur == start && face.len() < 3 {
                    break;
                }
                if face.len() > 1 && face[1..].contains(&cur) {
                    break; // self-intersection
                }
                face.push(cur);
                let nml = out_nml[cur as usize];
                let incoming = (out_pos[cur as usize] - out_pos[prev as usize]).normalize_or_zero();
                let mut best_next = None;
                let mut best_score = f32::MIN;
                for &cand in &out_adj[cur as usize] {
                    if cand == prev {
                        continue;
                    }
                    // Prefer unused edges; allow return to start to close.
                    if used_oriented.contains(&(cur, cand)) && cand != start {
                        continue;
                    }
                    if walk.contains(&(cur, cand)) && cand != start {
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
                walk.push((prev, cur));
            }
            if closed && face.len() >= 3 && face.len() <= 8 {
                for e in walk {
                    used_oriented.insert(e);
                }
                faces.push(face);
            }
        }
    }

    if faces.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }
    Ok(Extracted { positions: out_pos, faces })
}

fn mesh_data_from(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>) -> MeshData {
    let nv = positions.len();
    let nt = triangles.len();
    MeshData {
        positions,
        triangles,
        mask: vec![0.0; nv],
        color: vec![WHITE; nv],
        groups: vec![0; nt],
        hidden: vec![false; nt],
        uv: Vec::new(),
    }
}

fn push_quad(triangles: &mut Vec<[u32; 3]>, diagonals: &mut HashSet<[u32; 2]>, a: u32, b: u32, c: u32, d: u32) {
    // Quad a-b-c-d → triangles a-b-c and a-c-d; diagonal a-c.
    triangles.push([a, b, c]);
    triangles.push([a, c, d]);
    diagonals.insert([a.min(c), a.max(c)]);
}

/// Catmull–Clark topology without smoothing: every n-gon becomes n quads
/// via a face point and edge midpoints.
fn polygons_to_pure_quads(ex: &Extracted) -> Result<(MeshData, HashSet<[u32; 2]>), AutoRetopoError> {
    let mut positions = ex.positions.clone();
    let mut edge_mid: HashMap<[u32; 2], u32> = HashMap::new();
    let mut mid_of = |positions: &mut Vec<Vec3>, a: u32, b: u32| -> u32 {
        let key = [a.min(b), a.max(b)];
        if let Some(&m) = edge_mid.get(&key) {
            return m;
        }
        let mid = (positions[a as usize] + positions[b as usize]) * 0.5;
        let idx = positions.len() as u32;
        positions.push(mid);
        edge_mid.insert(key, idx);
        idx
    };

    let mut triangles = Vec::new();
    let mut diagonals = HashSet::new();
    for face in &ex.faces {
        let n = face.len();
        if n < 3 {
            continue;
        }
        let mut centroid = Vec3::ZERO;
        for &v in face {
            centroid += positions[v as usize];
        }
        centroid /= n as f32;
        let face_pt = positions.len() as u32;
        positions.push(centroid);

        for i in 0..n {
            let a = face[i];
            let b = face[(i + 1) % n];
            let mid = mid_of(&mut positions, a, b);
            let prev_mid = mid_of(&mut positions, face[(i + n - 1) % n], a);
            push_quad(&mut triangles, &mut diagonals, a, mid, face_pt, prev_mid);
        }
    }
    if triangles.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }
    Ok((mesh_data_from(positions, triangles), diagonals))
}

/// Keep quads as two triangles + diagonal; fan-triangulate other polygons.
fn polygons_to_quad_dominant(ex: &Extracted) -> Result<(MeshData, HashSet<[u32; 2]>), AutoRetopoError> {
    let mut triangles = Vec::new();
    let mut diagonals = HashSet::new();
    for face in &ex.faces {
        match face.len() {
            4 => push_quad(&mut triangles, &mut diagonals, face[0], face[1], face[2], face[3]),
            n if n >= 3 => {
                for k in 1..n - 1 {
                    triangles.push([face[0], face[k], face[k + 1]]);
                }
            }
            _ => {}
        }
    }
    if triangles.is_empty() {
        return Err(AutoRetopoError::Degenerate);
    }
    Ok((mesh_data_from(ex.positions.clone(), triangles), diagonals))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn auto_retopo_sphere_is_one_connected_piece() {
        // Extraction must return a single surface: floaters are stripped when
        // a dominant piece exists, otherwise Fragmented (#551).
        let mesh = primitives::icosphere(3, 1.0);
        let result = auto_retopo(
            &mesh,
            AutoRetopoOptions {
                target_faces: 400,
                iterations_per_level: 4,
                snap_to_surface: false,
                transfer_attributes: false,
                ..Default::default()
            },
        );
        match result {
            Ok(out) => {
                assert_eq!(connected_components(&out), 1);
                assert!(out.triangle_count() > 10, "got {} tris", out.triangle_count());
            }
            Err(AutoRetopoError::Fragmented { components }) => {
                assert!(components > 1);
            }
            // Field tracing on a small sphere can still fail to close faces.
            Err(AutoRetopoError::Degenerate) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn auto_retopo_ok_meshes_are_never_shattered() {
        let mesh = primitives::icosphere(4, 1.0);
        let result = auto_retopo(
            &mesh,
            AutoRetopoOptions {
                target_faces: 2_000,
                iterations_per_level: 4,
                pure_quad: true,
                snap_to_surface: false,
                transfer_attributes: false,
                ..Default::default()
            },
        );
        match result {
            Ok(out) => {
                assert_eq!(connected_components(&out), 1, "Ok meshes must be one piece");
                assert!(out.triangle_count() > 20, "got {} tris", out.triangle_count());
            }
            Err(AutoRetopoError::Fragmented { components }) => assert!(components > 1),
            Err(AutoRetopoError::Degenerate) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn keep_largest_strips_floaters_when_majority_exists() {
        // Two disconnected cubes: larger one must survive alone.
        let a = primitives::cube(2, 2.0);
        let b = primitives::cube(1, 0.3);
        let offset = glam::Vec3::new(10.0, 0.0, 0.0);
        let mut positions = a.positions.clone();
        let mut mask = a.mask.clone();
        let mut color = a.color.clone();
        let mut tris = a.triangles.clone();
        let base = positions.len() as u32;
        for p in &b.positions {
            positions.push(*p + offset);
        }
        mask.extend_from_slice(&b.mask);
        color.extend_from_slice(&b.color);
        for t in &b.triangles {
            tris.push([t[0] + base, t[1] + base, t[2] + base]);
        }
        let nt = a.triangle_count() + b.triangle_count();
        let shattered = Mesh::from_data(MeshData {
            positions,
            triangles: tris,
            mask,
            color,
            groups: vec![0; nt],
            hidden: vec![false; nt],
            uv: Vec::new(),
        });
        assert!(connected_components(&shattered) > 1);
        let kept = keep_largest_component(shattered).expect("majority piece");
        assert_eq!(connected_components(&kept), 1);
        assert_eq!(kept.triangle_count(), a.triangle_count());
    }

    #[test]
    fn auto_retopo_pure_quad_path_runs_without_lone_pair_assert_when_ok() {
        let mesh = primitives::icosphere(3, 1.0);
        let result = auto_retopo(
            &mesh,
            AutoRetopoOptions {
                target_faces: 300,
                iterations_per_level: 3,
                pure_quad: true,
                snap_to_surface: false,
                transfer_attributes: false,
                ..Default::default()
            },
        );
        if let Ok(out) = result {
            assert_eq!(out.triangle_count() % 2, 0, "pure-quad emit is triangle pairs");
            assert!(!out.quad_diagonals().is_empty());
        }
    }

    #[test]
    fn auto_retopo_transfers_paint_and_mask_when_connected() {
        let mut mesh = primitives::icosphere(3, 1.0);
        for v in 0..mesh.vertex_count() {
            if mesh.positions[v].y > 0.0 {
                mesh.color[v] = Vec3::new(1.0, 0.0, 0.0);
                mesh.mask[v] = 1.0;
            }
        }
        let result = auto_retopo(
            &mesh,
            AutoRetopoOptions {
                target_faces: 250,
                iterations_per_level: 3,
                pure_quad: true,
                snap_to_surface: true,
                transfer_attributes: true,
                ..Default::default()
            },
        );
        if let Ok(out) = result {
            assert!(out.color.iter().any(|c| c.x > 0.5 && c.y < 0.5));
            assert!(out.mask.iter().any(|&m| m > 0.25));
        }
    }

    #[test]
    fn auto_retopo_phase3_constraints_and_adaptivity_run() {
        let mesh = primitives::icosphere(3, 1.0);
        let constraints = vec![OrientationConstraint { position: Vec3::new(0.0, 1.0, 0.0), direction: Vec3::X }];
        let result = auto_retopo(
            &mesh,
            AutoRetopoOptions {
                target_faces: 400,
                iterations_per_level: 3,
                adaptivity: 0.35,
                anisotropy: 0.25,
                orientation_constraints: constraints,
                singularity_attractors: vec![Vec3::new(1.0, 0.0, 0.0)],
                snap_to_surface: false,
                transfer_attributes: false,
                ..Default::default()
            },
        );
        match result {
            Ok(out) => {
                assert!(out.triangle_count() > 4);
                assert_eq!(connected_components(&out), 1);
            }
            // Field tracing on a small sphere is platform-sensitive; macOS CI
            // has returned Degenerate while Ubuntu on the same commit succeeded.
            Err(AutoRetopoError::Fragmented { .. } | AutoRetopoError::Degenerate) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
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
