//! Retopology cage: a low-polygon mesh of explicit quads and triangles,
//! drawn by hand over a sculpt (RFC 0012).
//!
//! The cage is the editable source of a piece's mesh. [`Cage::build_mesh`]
//! triangulates it into an ordinary [`Mesh`] (two triangles per quad, with
//! the quads' diagonals recorded so quad counts and quad export are exact)
//! and renumbers the cage so that **cage vertex `i` is mesh vertex `i`**
//! for every vertex used by a face. Vertices used by no face ("dots",
//! placed before they become part of a face) come after them. While the
//! mesh keeps that topology ([`Mesh::topology_version`]), anything that
//! moves the mesh's vertices (a brush, an undo) can be copied back with
//! [`Cage::sync_from`].
//!
//! Faces are counter-clockwise seen from outside, with three or more
//! corners: triangles, quads and, for Poly Edit (RFC 0018), n-gons. Each
//! face carries a face group, which the built mesh's triangles get.
//!
//! The editing operations here are pure topology and geometry; snapping to
//! the sculpted surface is the caller's job.

use crate::{Mesh, MeshData};
use glam::Vec3;
use std::collections::{HashMap, HashSet};

/// The missing fourth corner of a triangle in the `.oxb` `CAGE` section,
/// which stores every face as four corners.
pub const NONE: u32 = u32::MAX;

/// A face of the cage: its corners in order (3 or more) and its face group.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Face {
    pub corners: Vec<u32>,
    pub group: u32,
}

impl Face {
    pub fn new(corners: Vec<u32>) -> Self {
        Face { corners, group: 0 }
    }

    pub fn with_group(corners: Vec<u32>, group: u32) -> Self {
        Face { corners, group }
    }

    pub fn len(&self) -> usize {
        self.corners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corners.is_empty()
    }

    /// The four `CAGE` corners of a triangle or quad (`NONE` fourth for a
    /// triangle); None for an n-gon.
    pub fn as_quad(&self) -> Option<[u32; 4]> {
        match self.corners[..] {
            [a, b, c] => Some([a, b, c, NONE]),
            [a, b, c, d] => Some([a, b, c, d]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cage {
    pub positions: Vec<Vec3>,
    pub faces: Vec<Face>,
    /// [`Mesh::topology_version`] of the mesh last built from (or linked
    /// to) this cage. Not saved.
    pub linked: Option<u64>,
    /// Pinned vertices, by index, kept in step with `positions` through
    /// every edit (shorter means the rest aren't pinned). Relax leaves
    /// them where they are; they can still be moved by hand.
    pub pinned: Vec<bool>,
}

/// The corners of a face.
pub fn corners(f: &Face) -> &[u32] {
    &f.corners
}

/// An undirected edge as `[low, high]`.
pub fn edge_key(a: u32, b: u32) -> [u32; 2] {
    [a.min(b), a.max(b)]
}

/// Newell normal of a polygon (not normalized; zero when degenerate).
pub fn polygon_normal(points: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    n
}

impl Cage {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn quad_count(&self) -> usize {
        self.faces.iter().filter(|f| f.len() == 4).count()
    }

    pub fn triangle_count(&self) -> usize {
        self.faces.iter().filter(|f| f.len() == 3).count()
    }

    /// Faces with more than four corners.
    pub fn ngon_count(&self) -> usize {
        self.faces.iter().filter(|f| f.len() > 4).count()
    }

    /// For each vertex, whether a face uses it.
    pub fn used(&self) -> Vec<bool> {
        let mut used = vec![false; self.positions.len()];
        for f in &self.faces {
            for &v in corners(f) {
                used[v as usize] = true;
            }
        }
        used
    }

    /// Vertices used by no face.
    pub fn dot_count(&self) -> usize {
        self.used().iter().filter(|u| !**u).count()
    }

    /// Faces of each undirected edge.
    pub fn edge_faces(&self) -> HashMap<[u32; 2], Vec<usize>> {
        let mut map: HashMap<[u32; 2], Vec<usize>> = HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            let c = corners(f);
            for i in 0..c.len() {
                map.entry(edge_key(c[i], c[(i + 1) % c.len()])).or_default().push(fi);
            }
        }
        map
    }

    /// Every undirected edge once.
    pub fn edges(&self) -> Vec<[u32; 2]> {
        let mut e: Vec<[u32; 2]> = self.edge_faces().into_keys().collect();
        e.sort_unstable();
        e
    }

    /// Edges with exactly one face.
    pub fn border_edges(&self) -> Vec<[u32; 2]> {
        let mut e: Vec<[u32; 2]> =
            self.edge_faces().into_iter().filter(|(_, f)| f.len() == 1).map(|(k, _)| k).collect();
        e.sort_unstable();
        e
    }

    /// Vertices a new face may use: dots and vertices on a border.
    pub fn open_vertices(&self) -> Vec<bool> {
        let mut open: Vec<bool> = self.used().iter().map(|u| !u).collect();
        for [a, b] in self.border_edges() {
            open[a as usize] = true;
            open[b as usize] = true;
        }
        open
    }

    pub fn is_pinned(&self, v: u32) -> bool {
        self.pinned.get(v as usize).copied().unwrap_or(false)
    }

    pub fn set_pinned(&mut self, v: u32, on: bool) {
        if (v as usize) < self.positions.len() && on != self.is_pinned(v) {
            self.pinned.resize(self.positions.len(), false);
            self.pinned[v as usize] = on;
            if !on && !self.pinned.iter().any(|p| *p) {
                self.pinned.clear();
            }
        }
    }

    pub fn pinned_count(&self) -> usize {
        self.pinned.iter().take(self.positions.len()).filter(|p| **p).count()
    }

    /// `pinned` padded to the vertex count, before an edit that removes or
    /// reorders vertices (it stays empty while nothing is pinned).
    fn fit_pins(&mut self) {
        if self.pinned.iter().any(|p| *p) {
            self.pinned.resize(self.positions.len(), false);
        } else {
            self.pinned.clear();
        }
    }

    pub fn add_vertex(&mut self, p: Vec3) -> u32 {
        self.fit_pins();
        if !self.pinned.is_empty() {
            self.pinned.push(false);
        }
        self.positions.push(p);
        (self.positions.len() - 1) as u32
    }

    /// Whether `verts` (3 or more distinct vertices, in order around the face)
    /// can become a face: not one already, and no edge would get a third
    /// face. Returns the corners in the winding to use: matching the
    /// neighbours across shared edges, or else facing along `outward`.
    pub fn face_winding(&self, verts: &[u32], outward: Vec3) -> Option<Vec<u32>> {
        if verts.len() < 3 || verts.iter().any(|&v| v as usize >= self.positions.len()) {
            return None;
        }
        let distinct: HashSet<u32> = verts.iter().copied().collect();
        if distinct.len() != verts.len() {
            return None;
        }
        let mut sorted: Vec<u32> = verts.to_vec();
        sorted.sort_unstable();
        if self.faces.iter().any(|f| {
            let mut c = corners(f).to_vec();
            c.sort_unstable();
            c == sorted
        }) {
            return None;
        }
        // Directed edges already used by faces.
        let mut directed: HashSet<(u32, u32)> = HashSet::new();
        let edge_faces = self.edge_faces();
        for f in &self.faces {
            let c = corners(f);
            for i in 0..c.len() {
                directed.insert((c[i], c[(i + 1) % c.len()]));
            }
        }
        let (mut same, mut opposite) = (0, 0);
        for i in 0..verts.len() {
            let (a, b) = (verts[i], verts[(i + 1) % verts.len()]);
            if edge_faces.get(&edge_key(a, b)).is_some_and(|f| f.len() >= 2) {
                return None;
            }
            if directed.contains(&(a, b)) {
                same += 1;
            }
            if directed.contains(&(b, a)) {
                opposite += 1;
            }
        }
        let mut out = verts.to_vec();
        if same > 0 && opposite > 0 {
            return None;
        }
        let flip = if same > 0 {
            true
        } else if opposite > 0 {
            false
        } else {
            let pts: Vec<Vec3> = verts.iter().map(|&v| self.positions[v as usize]).collect();
            polygon_normal(&pts).dot(outward) < 0.0
        };
        if flip {
            out.reverse();
        }
        Some(out)
    }

    /// Add a face through `verts` (see [`Self::face_winding`]). Returns its index.
    pub fn add_face(&mut self, verts: &[u32], outward: Vec3) -> Option<usize> {
        let w = self.face_winding(verts, outward)?;
        self.faces.push(Face::new(w));
        Some(self.faces.len() - 1)
    }

    pub fn remove_face(&mut self, face: usize) {
        if face < self.faces.len() {
            self.faces.remove(face);
        }
    }

    /// Remove vertex `v` and the faces using it. The last vertex takes its
    /// index.
    pub fn remove_vertex(&mut self, v: u32) {
        let n = self.positions.len() as u32;
        if v >= n {
            return;
        }
        self.faces.retain(|f| !corners(f).contains(&v));
        let last = n - 1;
        self.fit_pins();
        if !self.pinned.is_empty() {
            self.pinned.swap_remove(v as usize);
        }
        self.positions.swap_remove(v as usize);
        if v != last {
            for f in &mut self.faces {
                for c in f.corners.iter_mut() {
                    if *c == last {
                        *c = v;
                    }
                }
            }
        }
    }

    /// Merge vertex `gone` into `keep`: faces using `gone` use `keep`
    /// instead (a face that ends up with `keep` twice loses a corner; one
    /// left with fewer than three is removed), and `gone` is removed. Returns the
    /// index `keep` has afterwards.
    pub fn weld(&mut self, keep: u32, gone: u32) -> u32 {
        let n = self.positions.len() as u32;
        if keep == gone || keep >= n || gone >= n {
            return keep;
        }
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut c: Vec<u32> = corners(f).iter().map(|&x| if x == gone { keep } else { x }).collect();
            c.dedup();
            if c.len() > 1 && c.first() == c.last() {
                c.pop();
            }
            if c.len() >= 3 {
                faces.push(Face::with_group(c, f.group));
            }
        }
        // Two faces that became the same face: keep one.
        let mut seen = HashSet::new();
        faces.retain(|f| {
            let mut c = corners(f).to_vec();
            c.sort_unstable();
            seen.insert(c)
        });
        self.faces = faces;
        // The welded point stays pinned if either was.
        if self.is_pinned(gone) {
            self.set_pinned(keep, true);
        }
        let last = n - 1;
        self.remove_vertex(gone);
        if keep == last { gone } else { keep }
    }

    /// Insert an edge loop across the quad ring through edge `(a, b)`,
    /// cutting each crossed edge at `t` (0 at `a`, 1 at `b`, carried along
    /// the ring). The ring stops at borders and triangles; a triangle at
    /// an end is split so no T-junction is left. Returns the new vertices.
    pub fn insert_loop(&mut self, a: u32, b: u32, t: f32) -> Vec<u32> {
        let t = t.clamp(0.02, 0.98);
        let edge_faces = self.edge_faces();
        let Some(start) = edge_faces.get(&edge_key(a, b)) else { return Vec::new() };
        let mut mids: HashMap<[u32; 2], u32> = HashMap::new();
        let mut new_vertices = Vec::new();
        let mut visited: HashSet<usize> = HashSet::new();
        let mut new_faces: Vec<Face> = Vec::new();
        let mut replaced: HashMap<usize, Face> = HashMap::new();
        let mut ends: Vec<([u32; 2], usize)> = Vec::new();

        let mut mid = |cage: &mut Cage, u: u32, v: u32, t: f32| -> u32 {
            *mids.entry(edge_key(u, v)).or_insert_with(|| {
                let p = cage.positions[u as usize].lerp(cage.positions[v as usize], t);
                let id = cage.add_vertex(p);
                new_vertices.push(id);
                id
            })
        };

        for &first in start.clone().iter() {
            let (mut u, mut v, mut tt, mut face) = (a, b, t, first);
            loop {
                if !visited.insert(face) {
                    break;
                }
                let group = self.faces[face].group;
                let Some(f) = self.faces[face].as_quad().filter(|q| q[3] != NONE) else {
                    // A triangle (or an n-gon) ends the ring: split it through the cut.
                    ends.push(([u, v], face));
                    break;
                };
                // Rotate so the entry edge is corners 0-1.
                let Some(i) = (0..4).find(|&i| edge_key(f[i], f[(i + 1) % 4]) == edge_key(u, v)) else { break };
                let q = [f[i], f[(i + 1) % 4], f[(i + 2) % 4], f[(i + 3) % 4]];
                // Carry `t` in the direction q0 -> q1.
                let tq = if q[0] == u { tt } else { 1.0 - tt };
                let m0 = mid(self, q[0], q[1], tq);
                let m1 = mid(self, q[3], q[2], tq);
                replaced.insert(face, Face::with_group(vec![q[0], m0, m1, q[3]], group));
                new_faces.push(Face::with_group(vec![m0, q[1], q[2], m1], group));
                // Across the opposite edge q3 -> q2.
                let next = edge_faces.get(&edge_key(q[3], q[2])).and_then(|fs| fs.iter().copied().find(|&g| g != face));
                match next {
                    Some(g) => {
                        u = q[3];
                        v = q[2];
                        tt = tq;
                        face = g;
                    }
                    None => break,
                }
            }
        }
        for (fi, f) in replaced {
            self.faces[fi] = f;
        }
        self.faces.extend(new_faces);
        // Triangles at the ends: split into two through the new vertex.
        // An n-gon at an end just gets the new vertex as an extra corner.
        for ([u, v], fi) in ends {
            let Some(&m) = mids.get(&edge_key(u, v)) else { continue };
            let f = self.faces[fi].clone();
            let n = f.len();
            let Some(i) = (0..n).find(|&i| edge_key(f.corners[i], f.corners[(i + 1) % n]) == edge_key(u, v)) else {
                continue;
            };
            if n == 3 {
                let (p, q, r) = (f.corners[i], f.corners[(i + 1) % 3], f.corners[(i + 2) % 3]);
                self.faces[fi] = Face::with_group(vec![p, m, r], f.group);
                self.faces.push(Face::with_group(vec![m, q, r], f.group));
            } else {
                self.faces[fi].corners.insert(i + 1, m);
            }
        }
        new_vertices
    }

    /// Add a quad on border edge `(a, b)` reaching to two new vertices at
    /// `pa` (next to `a`) and `pb` (next to `b`). Returns the new face.
    pub fn extend(&mut self, a: u32, b: u32, pa: Vec3, pb: Vec3) -> Option<usize> {
        let edge_faces = self.edge_faces();
        if edge_faces.get(&edge_key(a, b)).is_none_or(|f| f.len() != 1) {
            return None;
        }
        let na = self.add_vertex(pa);
        let nb = self.add_vertex(pb);
        match self.add_face(&[a, b, nb, na], Vec3::ZERO) {
            Some(f) => Some(f),
            None => {
                self.positions.truncate(self.positions.len() - 2);
                self.pinned.truncate(self.positions.len());
                None
            }
        }
    }

    /// Move each vertex of `which` part of the way (`amount`, 0..=1)
    /// towards the middle of its neighbours. Vertices on a border, pinned
    /// vertices and dots stay. Returns the vertices that moved.
    pub fn relax(&mut self, which: &[u32], amount: f32) -> Vec<u32> {
        let amount = amount.clamp(0.0, 1.0);
        let edge_faces = self.edge_faces();
        let mut border = vec![false; self.positions.len()];
        let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); self.positions.len()];
        for (&[a, b], faces) in &edge_faces {
            if faces.len() == 1 {
                border[a as usize] = true;
                border[b as usize] = true;
            }
            neighbours[a as usize].push(b);
            neighbours[b as usize].push(a);
        }
        let targets: Vec<(u32, Vec3)> = which
            .iter()
            .filter(|&&v| (v as usize) < self.positions.len())
            .filter(|&&v| !border[v as usize] && !self.is_pinned(v) && !neighbours[v as usize].is_empty())
            .map(|&v| {
                let n = &neighbours[v as usize];
                let avg = n.iter().map(|&w| self.positions[w as usize]).sum::<Vec3>() / n.len() as f32;
                (v, self.positions[v as usize].lerp(avg, amount))
            })
            .collect();
        for &(v, p) in &targets {
            self.positions[v as usize] = p;
        }
        targets.into_iter().map(|(v, _)| v).collect()
    }

    /// Triangles for the faces (quads split along corners 0-2, n-gons by
    /// ear clipping), the quad diagonals, in cage indices, and each
    /// triangle's face group.
    pub fn triangulate(&self) -> (Vec<[u32; 3]>, HashSet<[u32; 2]>) {
        let (tris, diagonals, _) = self.triangulate_with_groups();
        (tris, diagonals)
    }

    /// [`Self::triangulate`] plus each triangle's face group.
    pub fn triangulate_with_groups(&self) -> (Vec<[u32; 3]>, HashSet<[u32; 2]>, Vec<u32>) {
        let mut tris = Vec::with_capacity(self.faces.len() * 2);
        let mut groups = Vec::with_capacity(self.faces.len() * 2);
        let mut diagonals = HashSet::new();
        for f in &self.faces {
            let c = &f.corners;
            match c.len() {
                0..=2 => continue,
                3 => tris.push([c[0], c[1], c[2]]),
                4 => {
                    tris.push([c[0], c[1], c[2]]);
                    tris.push([c[0], c[2], c[3]]);
                    diagonals.insert(edge_key(c[0], c[2]));
                }
                _ => tris.extend(ear_clip(&self.positions, c)),
            }
            groups.resize(tris.len(), f.group);
        }
        (tris, diagonals, groups)
    }

    /// Build the piece's mesh from the cage and renumber the cage so its
    /// vertex `i` is the mesh's vertex `i` (dots after). None without faces.
    pub fn build_mesh(&mut self) -> Option<Mesh> {
        self.build_mesh_with_order().map(|(m, _)| m)
    }

    /// [`Self::build_mesh`], also returning the renumbering: old cage
    /// vertex `v` is vertex `order[v]` afterwards.
    pub fn build_mesh_with_order(&mut self) -> Option<(Mesh, Vec<u32>)> {
        if self.faces.is_empty() {
            return None;
        }
        let (triangles, diagonals, groups) = self.triangulate_with_groups();
        let groups = if groups.iter().all(|&g| g == 0) { Vec::new() } else { groups };
        let mesh =
            Mesh::from_data(MeshData { positions: self.positions.clone(), triangles, groups, ..Default::default() });
        let perm = self.adopt_order(mesh.source_vertices());
        let diagonals = diagonals.into_iter().map(|[a, b]| edge_key(perm[a as usize], perm[b as usize])).collect();
        mesh.set_quad_diagonals(diagonals);
        self.linked = Some(mesh.topology_version());
        Some((mesh, perm))
    }

    /// Renumber so that cage vertex `source[i]` becomes `i`; the others
    /// (dots) follow in their order.
    fn adopt_order(&mut self, source: &[u32]) -> Vec<u32> {
        let n = self.positions.len();
        let mut perm = vec![u32::MAX; n];
        for (i, &s) in source.iter().enumerate() {
            if (s as usize) < n {
                perm[s as usize] = i as u32;
            }
        }
        let mut next = source.len() as u32;
        for p in perm.iter_mut() {
            if *p == u32::MAX {
                *p = next;
                next += 1;
            }
        }
        let mut positions = vec![Vec3::ZERO; n];
        for (old, &new) in perm.iter().enumerate() {
            positions[new as usize] = self.positions[old];
        }
        self.positions = positions;
        self.fit_pins();
        if !self.pinned.is_empty() {
            let mut pinned = vec![false; n];
            for (old, &new) in perm.iter().enumerate() {
                pinned[new as usize] = self.pinned[old];
            }
            self.pinned = pinned;
        }
        for f in &mut self.faces {
            for c in f.corners.iter_mut() {
                *c = perm[*c as usize];
            }
        }
        perm
    }

    /// Whether the cage still describes `mesh` (built from it, topology unchanged).
    pub fn is_linked_to(&self, mesh: &Mesh) -> bool {
        self.linked == Some(mesh.topology_version()) && mesh.vertex_count() <= self.positions.len()
    }

    /// Copy the mesh's vertex positions back into the cage (after a brush
    /// or an undo moved them). False if the cage no longer matches it.
    pub fn sync_from(&mut self, mesh: &Mesh) -> bool {
        if !self.is_linked_to(mesh) {
            return false;
        }
        self.positions[..mesh.vertex_count()].copy_from_slice(&mesh.positions);
        true
    }

    /// Link a cage loaded from a file, whose vertex `i` is vertex `i` of the
    /// saved mesh, to `mesh` built from that saved mesh. False (and the
    /// cage is unusable) if they don't match.
    pub fn relink(&mut self, mesh: &Mesh) -> bool {
        let used = self.used();
        let used_count = used.iter().filter(|u| **u).count();
        let (tris, diagonals) = self.triangulate();
        let source = mesh.source_vertices();
        let fits = used_count == mesh.vertex_count()
            && tris.len() == mesh.triangle_count()
            && used[..used_count.min(used.len())].iter().all(|u| *u)
            && source.iter().all(|&s| (s as usize) < used_count);
        if !fits {
            return false;
        }
        let perm = self.adopt_order(source);
        mesh.set_quad_diagonals(
            diagonals.into_iter().map(|[a, b]| edge_key(perm[a as usize], perm[b as usize])).collect(),
        );
        self.linked = Some(mesh.topology_version());
        true
    }
}

/// Split polygon `poly` (corner indices into `positions`, in order) into
/// `len - 2` triangles by ear clipping in the polygon's best-fit plane
/// (Newell normal). Falls back to a fan for degenerate polygons.
pub fn ear_clip(positions: &[Vec3], poly: &[u32]) -> Vec<[u32; 3]> {
    let pts: Vec<Vec3> = poly.iter().map(|&v| positions[v as usize]).collect();
    let n = polygon_normal(&pts);
    let fan = || (1..poly.len() - 1).map(|i| [poly[0], poly[i], poly[i + 1]]).collect();
    let Some(n) = n.try_normalize() else { return fan() };
    // 2D coordinates in the polygon's plane.
    let u = n.any_orthonormal_vector();
    let w = n.cross(u);
    let p2: Vec<glam::Vec2> = pts.iter().map(|p| glam::Vec2::new(p.dot(u), p.dot(w))).collect();
    let cross = |a: glam::Vec2, b: glam::Vec2, c: glam::Vec2| (b - a).perp_dot(c - a);
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    let mut out = Vec::with_capacity(poly.len() - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < poly.len() * poly.len() {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (p2[ia], p2[ib], p2[ic]);
            if cross(a, b, c) <= 1e-12 {
                continue; // reflex or flat corner
            }
            let inside = idx.iter().any(|&j| {
                j != ia
                    && j != ib
                    && j != ic
                    && cross(a, b, p2[j]) >= 0.0
                    && cross(b, c, p2[j]) >= 0.0
                    && cross(c, a, p2[j]) >= 0.0
            });
            if inside {
                continue;
            }
            out.push([poly[ia], poly[ib], poly[ic]]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            return fan();
        }
    }
    if idx.len() == 3 {
        out.push([poly[idx[0]], poly[idx[1]], poly[idx[2]]]);
    }
    if out.len() != poly.len() - 2 { fan() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 strip of quads in the XY plane, facing +Z:
    /// ```text
    /// 3---4---5
    /// |   |   |
    /// 0---1---2
    /// ```
    fn strip() -> Cage {
        let mut c = Cage::default();
        for y in 0..2 {
            for x in 0..3 {
                c.add_vertex(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        c.add_face(&[0, 1, 4, 3], Vec3::Z).unwrap();
        c.add_face(&[1, 2, 5, 4], Vec3::Z).unwrap();
        c
    }

    fn normal_of(c: &Cage, f: usize) -> Vec3 {
        let pts: Vec<Vec3> = corners(&c.faces[f]).iter().map(|&v| c.positions[v as usize]).collect();
        polygon_normal(&pts).normalize()
    }

    #[test]
    fn faces_wind_outward_and_match_neighbours() {
        let mut c = Cage::default();
        for p in [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y] {
            c.add_vertex(p);
        }
        // Given clockwise but asked to face +Z: flipped.
        let f = c.add_face(&[0, 3, 2, 1], Vec3::Z).unwrap();
        assert!(normal_of(&c, f).dot(Vec3::Z) > 0.99);
        // A neighbour drawn the "wrong" way follows the shared edge instead
        // of the hint.
        c.add_vertex(Vec3::new(2.0, 0.0, 0.0));
        c.add_vertex(Vec3::new(2.0, 1.0, 0.0));
        let g = c.add_face(&[1, 2, 5, 4], -Vec3::Z).unwrap();
        assert!(normal_of(&c, g).dot(Vec3::Z) > 0.99);
        // The same face twice, repeated corners, or a third face on an edge: refused.
        assert!(c.add_face(&[0, 1, 2, 3], Vec3::Z).is_none());
        assert!(c.add_face(&[0, 0, 1], Vec3::Z).is_none());
        let t = c.add_vertex(Vec3::new(1.0, 0.5, 1.0));
        assert!(c.add_face(&[1, 2, t], Vec3::Z).is_none(), "edge 1-2 already has two faces");
        assert_eq!(c.quad_count(), 2);
        assert_eq!(c.dot_count(), 1);
    }

    #[test]
    fn borders_and_open_vertices() {
        let mut c = strip();
        assert_eq!(c.border_edges().len(), 6);
        assert_eq!(c.edges().len(), 7);
        let dot = c.add_vertex(Vec3::new(5.0, 5.0, 0.0));
        let open = c.open_vertices();
        assert!(open.iter().all(|o| *o), "every vertex of a strip is on its border, and dots are open");
        assert!(open[dot as usize]);
    }

    #[test]
    fn build_mesh_aligns_indices_and_records_quads() {
        let mut c = strip();
        let dot = c.add_vertex(Vec3::new(9.0, 9.0, 9.0));
        let dot_pos = c.positions[dot as usize];
        let mesh = c.build_mesh().unwrap();
        assert_eq!(mesh.vertex_count(), 6);
        assert_eq!(mesh.triangle_count(), 4);
        assert_eq!(mesh.quad_count(), 2, "exact quads, not guessed");
        for i in 0..6 {
            assert_eq!(c.positions[i], mesh.positions[i], "cage vertex i is mesh vertex i");
        }
        assert_eq!(c.positions[6], dot_pos, "the dot comes after the mesh's vertices");
        assert!(c.is_linked_to(&mesh));
        // A brush moves the mesh; the cage follows.
        let mut moved = mesh.clone();
        moved.positions[2].z = 0.5;
        assert!(c.sync_from(&moved));
        assert_eq!(c.positions[2].z, 0.5);
        // A different mesh (new topology) doesn't match.
        let other = strip().build_mesh().unwrap();
        assert!(!c.is_linked_to(&other));
        assert!(!c.sync_from(&other));
        // Faces of the mesh face the same way as the cage's.
        for t in &mesh.triangles {
            let [a, b, d] = t.map(|i| mesh.positions[i as usize]);
            assert!((b - a).cross(d - a).z > 0.0);
        }
    }

    #[test]
    fn relink_after_saving_and_loading() {
        let mut c = strip();
        c.add_vertex(Vec3::new(4.0, 4.0, 0.0));
        let mesh = c.build_mesh().unwrap();
        // Saved: the mesh's data in its order, and the cage aligned with it.
        let saved = mesh.to_data();
        let mut loaded_cage = Cage { linked: None, ..c.clone() };
        let loaded = Mesh::from_data(saved);
        assert!(loaded_cage.relink(&loaded));
        assert_eq!(loaded.quad_count(), 2);
        for i in 0..loaded.vertex_count() {
            assert_eq!(loaded_cage.positions[i], loaded.positions[i]);
        }
        assert_eq!(loaded_cage.dot_count(), 1);
        // A cage that doesn't fit the mesh is refused.
        let mut wrong = Cage::default();
        wrong.add_vertex(Vec3::ZERO);
        assert!(!wrong.relink(&loaded));
    }

    #[test]
    fn weld_and_remove() {
        let mut c = strip();
        let d = c.add_vertex(Vec3::new(3.0, 0.0, 0.0));
        // Welding a dot into a corner just removes the dot.
        let keep = c.weld(2, d);
        assert_eq!(keep, 2);
        assert_eq!(c.positions.len(), 6);
        // Welding two corners of one quad turns it into a triangle.
        c.weld(1, 2);
        assert_eq!(c.quad_count(), 1);
        assert_eq!(c.triangle_count(), 1);
        // Removing a vertex removes its faces.
        let mut c = strip();
        c.remove_vertex(0);
        assert_eq!(c.faces.len(), 1);
        assert_eq!(c.positions.len(), 5);
        assert!(c.faces.iter().all(|f| corners(f).iter().all(|&v| (v as usize) < 5)));
    }

    #[test]
    fn insert_loop_cuts_the_whole_ring() {
        // Cut across both quads through the middle row (edge 0-3 runs up).
        let mut c = strip();
        let new = c.insert_loop(0, 3, 0.5);
        assert_eq!(new.len(), 3, "one new vertex on each of the three vertical edges");
        assert_eq!(c.quad_count(), 4);
        for &v in &new {
            assert!((c.positions[v as usize].y - 0.5).abs() < 1e-6);
        }
        for f in 0..c.faces.len() {
            assert!(normal_of(&c, f).dot(Vec3::Z) > 0.99, "winding kept");
        }
        // The ring is closed cleanly: every interior edge has two faces.
        let ef = c.edge_faces();
        assert_eq!(ef.values().filter(|f| f.len() == 2).count(), 4, "a 2×2 grid has four inner edges");
        // Cutting the other way (edge 0-1) only crosses one quad.
        let mut c = strip();
        let new = c.insert_loop(1, 0, 0.25);
        assert_eq!(new.len(), 2);
        assert_eq!(c.quad_count(), 3);
        // t is measured from the first vertex given: 0.25 from vertex 1.
        assert!(new.iter().any(|&v| (c.positions[v as usize] - Vec3::new(0.75, 0.0, 0.0)).length() < 1e-6));
    }

    #[test]
    fn insert_loop_splits_a_triangle_at_the_end() {
        let mut c = strip();
        let apex = c.add_vertex(Vec3::new(3.0, 0.5, 0.0));
        c.add_face(&[2, apex, 5], Vec3::Z).unwrap();
        c.insert_loop(0, 3, 0.5);
        assert_eq!(c.quad_count(), 4);
        assert_eq!(c.triangle_count(), 2, "the end triangle is split, no T-junction");
        let ef = c.edge_faces();
        assert!(ef.values().all(|f| f.len() <= 2));
        // Every edge either is on the border or has two faces; the new
        // vertex on edge 2-5 is shared by four faces.
        let open = ef.values().filter(|f| f.len() == 1).count();
        assert_eq!(open, 8);
    }

    #[test]
    fn extend_adds_a_quad_on_a_border_edge() {
        let mut c = strip();
        let f = c.extend(2, 5, Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 0.0)).unwrap();
        assert!(normal_of(&c, f).dot(Vec3::Z) > 0.99);
        assert_eq!(c.quad_count(), 3);
        // Not on an inner edge.
        assert!(c.extend(1, 4, Vec3::ZERO, Vec3::ONE).is_none());
        assert_eq!(c.positions.len(), 8);
    }

    #[test]
    fn relax_moves_inner_vertices_only() {
        // A 3×3 grid with its middle vertex pulled aside.
        let mut c = Cage::default();
        for y in 0..3 {
            for x in 0..3 {
                c.add_vertex(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        for (a, b, d, e) in [(0, 1, 4, 3), (1, 2, 5, 4), (3, 4, 7, 6), (4, 5, 8, 7)] {
            c.add_face(&[a, b, d, e], Vec3::Z).unwrap();
        }
        c.positions[4] = Vec3::new(1.6, 1.4, 0.0);
        let all: Vec<u32> = (0..9).collect();
        let moved = c.relax(&all, 1.0);
        assert_eq!(moved, vec![4]);
        assert!((c.positions[4] - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-6);
        assert_eq!(c.positions[0], Vec3::ZERO);
        // Pinned, the middle vertex stays where it is.
        c.positions[4] = Vec3::new(1.6, 1.4, 0.0);
        c.set_pinned(4, true);
        assert!(c.relax(&all, 1.0).is_empty());
        assert_eq!(c.positions[4], Vec3::new(1.6, 1.4, 0.0));
    }

    #[test]
    fn pins_follow_their_points_through_edits() {
        let pinned_at = |c: &Cage| -> Vec<Vec3> {
            let mut p: Vec<Vec3> =
                (0..c.positions.len() as u32).filter(|&v| c.is_pinned(v)).map(|v| c.positions[v as usize]).collect();
            p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            p
        };
        let mut c = strip();
        c.set_pinned(1, true);
        c.set_pinned(5, true);
        let expect = pinned_at(&c);
        assert_eq!(c.pinned_count(), 2);
        // New points, an edge loop and a removed point renumber vertices;
        // the pins stay on the same positions.
        c.add_vertex(Vec3::new(9.0, 9.0, 0.0));
        c.insert_loop(0, 3, 0.5);
        assert_eq!(pinned_at(&c), expect);
        let loose = c.add_vertex(Vec3::new(-5.0, 0.0, 0.0));
        c.remove_vertex(0);
        assert_eq!(pinned_at(&c), expect);
        // Building the mesh renumbers the cage to the mesh's order.
        c.build_mesh().unwrap();
        assert_eq!(pinned_at(&c), expect);
        // Welding a pinned point into another keeps it pinned.
        let n = c.positions.len() as u32;
        let a = (0..n).find(|&v| c.is_pinned(v)).unwrap();
        let b = (0..n).find(|&v| !c.is_pinned(v) && c.positions[v as usize] != Vec3::new(-5.0, 0.0, 0.0)).unwrap();
        let kept = c.weld(b, a);
        assert!(c.is_pinned(kept));
        assert_eq!(c.pinned_count(), 2);
        let _ = loose;
        // Unpinning the last pin leaves nothing pinned (and equal to a fresh cage).
        for v in 0..c.positions.len() as u32 {
            c.set_pinned(v, false);
        }
        assert!(c.pinned.is_empty());
    }
}
