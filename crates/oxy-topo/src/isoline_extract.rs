//! Autoremesher-style UV-isoline quad extraction (MIT-inspired).
//!
//! Instant Meshes dual-graph face walks leave Swiss-cheese holes on closed
//! characters (#551). Autoremesher ([huxingyi/autoremesher](https://github.com/huxingyi/autoremesher),
//! MIT) extracts quads from integer isolines of a per-triangle UV field, then
//! collapses short edges, fills short holes, and drops non-manifold faces.
//!
//! UVs are supplied by the caller (Instant Meshes integer-jump chart). Pure
//! Rust; Instant Meshes reference C++ was not opened (GPL). See `CLEAN_ROOM.md`.

use glam::{Vec2, Vec3};
use std::collections::{HashMap, HashSet};

/// Oriented polygon loops in output vertex indices.
pub struct IsolineMesh {
    pub positions: Vec<Vec3>,
    pub faces: Vec<Vec<u32>>,
}

/// Extract a quad-dominant mesh from vertex UVs via Autoremesher-style isolines.
pub fn extract_isoline_mesh(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    vertex_uvs: &[Vec2],
    rho: f32,
) -> Option<IsolineMesh> {
    if positions.len() < 4 || triangles.is_empty() || vertex_uvs.len() != positions.len() || rho <= 1e-8 {
        return None;
    }

    let corner_uvs: Vec<[Vec2; 3]> = triangles
        .iter()
        .map(|t| [vertex_uvs[t[0] as usize], vertex_uvs[t[1] as usize], vertex_uvs[t[2] as usize]])
        .collect();

    let mut cross_points: Vec<Vec3> = Vec::new();
    let mut cross_src: Vec<usize> = Vec::new();
    let mut point_key: HashMap<[i64; 3], u32> = HashMap::new();
    // Tighter weld than Autoremesher's default proximity — a loose quant +
    // short-edge collapse merged the whole isoline graph into a handful of points.
    let quant = (rho * 0.002).max(1e-7);

    let mut add_point = |p: Vec3, tri: usize| -> u32 {
        let key = [(p.x / quant).round() as i64, (p.y / quant).round() as i64, (p.z / quant).round() as i64];
        if let Some(&idx) = point_key.get(&key) {
            return idx;
        }
        let idx = cross_points.len() as u32;
        point_key.insert(key, idx);
        cross_points.push(p);
        cross_src.push(tri);
        idx
    };

    let mut connections: HashSet<(u32, u32)> = HashSet::new();
    for (ti, tri) in triangles.iter().enumerate() {
        let uvs = corner_uvs[ti];
        let verts = [positions[tri[0] as usize], positions[tri[1] as usize], positions[tri[2] as usize]];
        extract_triangle_connections(verts, uvs, ti, &mut add_point, &mut connections);
    }
    if connections.is_empty() || cross_points.len() < 4 {
        return None;
    }

    let mut adj: Vec<HashSet<u32>> = vec![HashSet::new(); cross_points.len()];
    for &(a, b) in &connections {
        if a == b {
            continue;
        }
        adj[a as usize].insert(b);
        adj[b as usize].insert(a);
    }

    collapse_short_edges(&mut cross_points, &mut adj, (rho * 0.02).max(1e-7));
    // One dangling pass only — aggressive pruning starved face cycles.
    remove_dangling(&mut adj);

    let mut faces = extract_cycles(&cross_points, &adj, &cross_src, triangles, positions);
    if faces.is_empty() {
        return None;
    }

    let mut half_edges = rebuild_half_edges(&faces);
    fix_short_holes(&cross_points, &mut faces, &mut half_edges);
    faces = keep_largest_faces(faces);
    faces = remove_nonmanifold_faces(faces);
    if faces.is_empty() {
        return None;
    }

    let mut used = vec![false; cross_points.len()];
    for f in &faces {
        for &v in f {
            used[v as usize] = true;
        }
    }
    let mut remap = vec![u32::MAX; cross_points.len()];
    let mut new_pos = Vec::new();
    for (i, &u) in used.iter().enumerate() {
        if !u {
            continue;
        }
        remap[i] = new_pos.len() as u32;
        new_pos.push(cross_points[i]);
    }
    for f in &mut faces {
        for v in f.iter_mut() {
            *v = remap[*v as usize];
        }
        f.retain(|&v| v != u32::MAX);
    }
    faces.retain(|f| f.len() >= 3);

    if faces.is_empty() || new_pos.len() < 4 {
        return None;
    }
    Some(IsolineMesh { positions: new_pos, faces })
}

fn uv_comp(uv: Vec2, coord: usize) -> f32 {
    if coord == 0 { uv.x } else { uv.y }
}

fn extract_triangle_connections(
    verts: [Vec3; 3],
    uvs: [Vec2; 3],
    tri: usize,
    add_point: &mut impl FnMut(Vec3, usize) -> u32,
    connections: &mut HashSet<(u32, u32)>,
) {
    for coord in 0..2 {
        let mut hits: HashMap<i32, Vec<(Vec3, Vec2)>> = HashMap::new();
        for e in 0..3 {
            let j = (e + 1) % 3;
            let (ua, ub) = (uv_comp(uvs[e], coord), uv_comp(uvs[j], coord));
            let (pa, pb) = (verts[e], verts[j]);
            let (uva, uvb) = (uvs[e], uvs[j]);
            if (ua - ua.round()).abs() < 1e-4 && (ua - ub).abs() < 1e-4 {
                let integer = ua.round() as i32;
                hits.entry(integer).or_default().push((pa, uva));
                hits.entry(integer).or_default().push((pb, uvb));
                continue;
            }
            let lo = ua.min(ub);
            let hi = ua.max(ub);
            let mut k = lo.ceil() as i32;
            let k_end = hi.floor() as i32;
            while k <= k_end {
                let denom = ub - ua;
                if denom.abs() < 1e-8 {
                    break;
                }
                let t = (k as f32 - ua) / denom;
                if (-1e-4..=1.0 + 1e-4).contains(&t) {
                    let t = t.clamp(0.0, 1.0);
                    let p = pa.lerp(pb, t);
                    let uv = uva.lerp(uvb, t);
                    hits.entry(k).or_default().push((p, uv));
                }
                k += 1;
            }
        }
        for (_integer, pts) in hits {
            if pts.len() < 2 {
                continue;
            }
            let mut pts = pts;
            let other = 1 - coord;
            pts.sort_by(|a, b| {
                uv_comp(a.1, other).partial_cmp(&uv_comp(b.1, other)).unwrap_or(std::cmp::Ordering::Equal)
            });
            for w in pts.windows(2) {
                let a = add_point(w[0].0, tri);
                let b = add_point(w[1].0, tri);
                if a != b {
                    let e = if a < b { (a, b) } else { (b, a) };
                    connections.insert(e);
                }
            }
        }
    }
}

fn collapse_short_edges(points: &mut [Vec3], adj: &mut [HashSet<u32>], eps: f32) {
    let eps2 = eps * eps;
    let n = points.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let find = |parent: &mut [u32], mut x: u32| {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    };
    let mut short: Vec<(f32, u32, u32)> = Vec::new();
    for (i, nbrs) in adj.iter().enumerate().take(n) {
        for &j in nbrs {
            if (i as u32) < j {
                let d = points[i].distance_squared(points[j as usize]);
                if d <= eps2 {
                    short.push((d, i as u32, j));
                }
            }
        }
    }
    short.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for &(_, a, b) in &short {
        let pa = find(&mut parent, a);
        let pb = find(&mut parent, b);
        if pa != pb {
            parent[pa as usize] = pb;
            let mid = (points[pa as usize] + points[pb as usize]) * 0.5;
            points[pb as usize] = mid;
        }
    }
    let mut new_adj: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    for (i, nbrs) in adj.iter().enumerate() {
        let pi = find(&mut parent, i as u32);
        for &j in nbrs {
            let pj = find(&mut parent, j);
            if pi != pj {
                new_adj[pi as usize].insert(pj);
                new_adj[pj as usize].insert(pi);
            }
        }
    }
    for (slot, src) in adj.iter_mut().zip(new_adj.iter_mut()) {
        *slot = std::mem::take(src);
    }
}

fn remove_dangling(adj: &mut [HashSet<u32>]) {
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..adj.len() {
            if adj[i].len() == 1 {
                let n = *adj[i].iter().next().unwrap();
                adj[i].clear();
                adj[n as usize].remove(&(i as u32));
                changed = true;
            }
        }
    }
}

fn extract_cycles(
    points: &[Vec3],
    adj: &[HashSet<u32>],
    cross_src: &[usize],
    triangles: &[[u32; 3]],
    mesh_pos: &[Vec3],
) -> Vec<Vec<u32>> {
    let n = points.len();
    let mut normals = vec![Vec3::Y; n];
    for (i, &ti) in cross_src.iter().enumerate().take(n) {
        if let Some(t) = triangles.get(ti) {
            let (a, b, c) = (mesh_pos[t[0] as usize], mesh_pos[t[1] as usize], mesh_pos[t[2] as usize]);
            normals[i] = (b - a).cross(c - a).normalize_or_zero();
            if normals[i] == Vec3::ZERO {
                normals[i] = Vec3::Y;
            }
        }
    }

    let mut faces = Vec::new();
    let mut used: HashSet<(u32, u32)> = HashSet::new();
    for start in 0..n as u32 {
        let neighbors: Vec<u32> = adj[start as usize].iter().copied().collect();
        for next in neighbors {
            if used.contains(&(start, next)) {
                continue;
            }
            let mut face = vec![start];
            let mut walk = Vec::new();
            let mut prev = start;
            let mut cur = next;
            walk.push((prev, cur));
            let mut closed = false;
            for _ in 0..16 {
                if cur == start && face.len() >= 3 {
                    closed = true;
                    break;
                }
                if face.len() > 1 && face[1..].contains(&cur) {
                    break;
                }
                face.push(cur);
                let nml = normals[cur as usize];
                let incoming = (points[cur as usize] - points[prev as usize]).normalize_or_zero();
                let mut best = None;
                let mut best_score = f32::MIN;
                for &cand in &adj[cur as usize] {
                    if cand == prev {
                        continue;
                    }
                    if used.contains(&(cur, cand)) && cand != start {
                        continue;
                    }
                    let outgoing = (points[cand as usize] - points[cur as usize]).normalize_or_zero();
                    let cross = incoming.cross(outgoing).dot(nml);
                    let dot = incoming.dot(outgoing).clamp(-1.0, 1.0);
                    let score = cross.atan2(dot);
                    if score > best_score {
                        best_score = score;
                        best = Some(cand);
                    }
                }
                let Some(nxt) = best else { break };
                prev = cur;
                cur = nxt;
                walk.push((prev, cur));
            }
            if closed && (3..=6).contains(&face.len()) {
                let mut area_n = Vec3::ZERO;
                for i in 0..face.len() {
                    let a = points[face[i] as usize];
                    let b = points[face[(i + 1) % face.len()] as usize];
                    area_n += a.cross(b);
                }
                let src_n = normals[face[0] as usize];
                if area_n.dot(src_n) < 0.0 {
                    face.reverse();
                    walk = walk.into_iter().map(|(a, b)| (b, a)).rev().collect();
                }
                for e in walk {
                    used.insert(e);
                }
                faces.push(face);
            }
        }
    }
    faces
}

fn rebuild_half_edges(faces: &[Vec<u32>]) -> HashSet<(u32, u32)> {
    let mut he = HashSet::new();
    for f in faces {
        let n = f.len();
        for i in 0..n {
            he.insert((f[i], f[(i + 1) % n]));
        }
    }
    he
}

fn fix_short_holes(points: &[Vec3], faces: &mut Vec<Vec<u32>>, half_edges: &mut HashSet<(u32, u32)>) {
    let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in half_edges.iter() {
        if !half_edges.contains(&(b, a)) {
            next.entry(a).or_default().push(b);
        }
    }
    let mut visited: HashSet<u32> = HashSet::new();
    let keys: Vec<u32> = next.keys().copied().collect();
    for start in keys {
        if visited.contains(&start) {
            continue;
        }
        let mut loop_v = Vec::new();
        let mut cur = start;
        let mut ok = false;
        for _ in 0..66 {
            if visited.contains(&cur) {
                break;
            }
            visited.insert(cur);
            loop_v.push(cur);
            let Some(nbrs) = next.get(&cur) else { break };
            if nbrs.len() != 1 {
                break;
            }
            let nxt = nbrs[0];
            if nxt == start && loop_v.len() >= 3 {
                ok = true;
                break;
            }
            cur = nxt;
        }
        if !ok || loop_v.len() > 64 {
            continue;
        }
        fill_hole_with_quads(points, &mut loop_v, faces, half_edges);
    }
}

fn fill_hole_with_quads(
    points: &[Vec3],
    hole: &mut Vec<u32>,
    faces: &mut Vec<Vec<u32>>,
    half_edges: &mut HashSet<(u32, u32)>,
) {
    while hole.len() > 2 {
        if hole.len() == 3 {
            let f = vec![hole[2], hole[1], hole[0]];
            record_face(&f, faces, half_edges);
            return;
        }
        if hole.len() == 4 {
            let f = vec![hole[3], hole[2], hole[1], hole[0]];
            record_face(&f, faces, half_edges);
            return;
        }
        let mut scores: Vec<(usize, f32)> = Vec::with_capacity(hole.len());
        for i in 0..hole.len() {
            let h = (i + hole.len() - 1) % hole.len();
            let j = (i + 1) % hole.len();
            let k = (j + 1) % hole.len();
            let left = (points[hole[h] as usize] - points[hole[i] as usize]).normalize_or_zero();
            let right = (points[hole[k] as usize] - points[hole[j] as usize]).normalize_or_zero();
            scores.push((i, left.dot(right)));
        }
        scores.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut changed = false;
        for &(i, score) in scores.iter().rev() {
            if score <= 0.0 {
                break;
            }
            let h = (i + hole.len() - 1) % hole.len();
            let j = (i + 1) % hole.len();
            let k = (j + 1) % hole.len();
            let candidate = [hole[k], hole[j], hole[i], hole[h]];
            let edges = [
                (candidate[0], candidate[1]),
                (candidate[1], candidate[2]),
                (candidate[2], candidate[3]),
                (candidate[3], candidate[0]),
            ];
            if edges.iter().any(|e| half_edges.contains(e)) {
                continue;
            }
            record_face(&candidate, faces, half_edges);
            let mut new_hole = Vec::with_capacity(hole.len() - 2);
            for (w, &v) in hole.iter().enumerate() {
                if w != i && w != j {
                    new_hole.push(v);
                }
            }
            *hole = new_hole;
            changed = true;
            break;
        }
        if !changed {
            break;
        }
    }
}

fn record_face(face: &[u32], faces: &mut Vec<Vec<u32>>, half_edges: &mut HashSet<(u32, u32)>) {
    let n = face.len();
    for i in 0..n {
        half_edges.insert((face[i], face[(i + 1) % n]));
    }
    faces.push(face.to_vec());
}

fn keep_largest_faces(faces: Vec<Vec<u32>>) -> Vec<Vec<u32>> {
    if faces.is_empty() {
        return faces;
    }
    let n = faces.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut [usize], mut x: usize| {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    };
    let mut edge_face: HashMap<[u32; 2], usize> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for i in 0..f.len() {
            let a = f[i];
            let b = f[(i + 1) % f.len()];
            let key = [a.min(b), a.max(b)];
            if let Some(&other) = edge_face.get(&key) {
                let (pa, pb) = (find(&mut parent, fi), find(&mut parent, other));
                parent[pa] = pb;
            } else {
                edge_face.insert(key, fi);
            }
        }
    }
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for i in 0..n {
        *sizes.entry(find(&mut parent, i)).or_default() += 1;
    }
    let (main, _) = sizes.into_iter().max_by_key(|(_, s)| *s).unwrap();
    faces.into_iter().enumerate().filter(|(i, _)| find(&mut parent, *i) == main).map(|(_, f)| f).collect()
}

fn remove_nonmanifold_faces(faces: Vec<Vec<u32>>) -> Vec<Vec<u32>> {
    let mut edge_count: HashMap<[u32; 2], u32> = HashMap::new();
    for f in &faces {
        for i in 0..f.len() {
            let a = f[i];
            let b = f[(i + 1) % f.len()];
            let key = [a.min(b), a.max(b)];
            *edge_count.entry(key).or_default() += 1;
        }
    }
    let mut open_at: HashMap<u32, u32> = HashMap::new();
    for (key, &c) in &edge_count {
        if c == 1 {
            *open_at.entry(key[0]).or_default() += 1;
            *open_at.entry(key[1]).or_default() += 1;
        }
    }
    faces.into_iter().filter(|f| !f.iter().any(|v| open_at.get(v).copied().unwrap_or(0) > 2)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isoline_on_flat_grid_makes_faces() {
        let mut positions = Vec::new();
        let mut uvs = Vec::new();
        for y in 0..3 {
            for x in 0..3 {
                positions.push(Vec3::new(x as f32, y as f32, 0.0));
                uvs.push(Vec2::new(x as f32, y as f32));
            }
        }
        let mut tris = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                let i = (y * 3 + x) as u32;
                tris.push([i, i + 1, i + 3]);
                tris.push([i + 1, i + 4, i + 3]);
            }
        }
        let out = extract_isoline_mesh(&positions, &tris, &uvs, 1.0);
        assert!(out.is_some(), "expected isoline faces on a flat grid");
        assert!(!out.unwrap().faces.is_empty());
    }
}
