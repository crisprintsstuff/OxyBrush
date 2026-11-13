//! Poly Edit (RFC 0018): polygon modeling operations on a [`Cage`].
//!
//! Everything here is pure topology and geometry on the cage: which faces
//! an action reaches, and the edits themselves. The app picks elements,
//! records undo and builds the piece's mesh from the result.
//!
//! Interactive actions are split in two. The topology change happens once,
//! at the press, with every new point placed where it starts (an extrude's
//! cap on the original face, an inset's inner ring on the outer one). What
//! a drag changes is only how far those points have gone: [`Slide`] holds
//! each point's start and direction, and [`Slide::apply`] places them for
//! an amount. That keeps a drag cheap and makes Escape a matter of
//! sliding back to zero.
//!
//! INVARIANT (checked by [`validate`] in the tests): faces have at least
//! three distinct corners in range, every edge has at most two faces, and
//! two faces that share an edge run it in opposite directions (consistent
//! winding). Every operation keeps each face's group, and gives the faces
//! it creates a new one.

use crate::retopo::{Cage, Face, corners, edge_key, polygon_normal};
use glam::Vec3;
use std::collections::{HashMap, HashSet, VecDeque};

/// How far an action spreads from the face it's applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Just that face.
    One,
    /// The face loop across the edge nearest the pointer (quads only).
    Loop,
    /// Every face in the same face group.
    Group,
    /// The faces of the same group connected to it.
    GroupIsland,
    /// Connected faces facing within `FLAT_DEGREES` of it.
    Flat,
    /// Every face connected to it.
    Island,
}

impl Reach {
    pub const ALL: [Reach; 6] = [Reach::One, Reach::Loop, Reach::Group, Reach::GroupIsland, Reach::Flat, Reach::Island];

    pub fn label(self) -> &'static str {
        match self {
            Reach::One => "This face",
            Reach::Loop => "Face loop",
            Reach::Group => "Face group",
            Reach::GroupIsland => "Face group, this island",
            Reach::Flat => "Flat region",
            Reach::Island => "Island",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Reach::One => "one",
            Reach::Loop => "loop",
            Reach::Group => "group",
            Reach::GroupIsland => "group_island",
            Reach::Flat => "flat",
            Reach::Island => "island",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == k)
    }
}

/// Faces within this angle of the clicked one count as one flat region.
pub const FLAT_DEGREES: f32 = 5.0;

/// The faces sharing each edge, for walking from face to face.
pub struct Adjacency {
    edge_faces: HashMap<[u32; 2], Vec<usize>>,
}

impl Adjacency {
    pub fn new(c: &Cage) -> Self {
        Adjacency { edge_faces: c.edge_faces() }
    }

    /// The faces across each edge of `face`.
    pub fn neighbours<'a>(&'a self, c: &'a Cage, face: usize) -> impl Iterator<Item = usize> + 'a {
        let k = corners(&c.faces[face]);
        (0..k.len()).flat_map(move |i| {
            self.edge_faces
                .get(&edge_key(k[i], k[(i + 1) % k.len()]))
                .into_iter()
                .flatten()
                .copied()
                .filter(move |&g| g != face)
        })
    }

    pub fn faces_of_edge(&self, a: u32, b: u32) -> &[usize] {
        self.edge_faces.get(&edge_key(a, b)).map_or(&[], |v| &v[..])
    }
}

/// Unit normal of face `f` (zero when degenerate).
pub fn face_normal(c: &Cage, f: usize) -> Vec3 {
    let pts: Vec<Vec3> = corners(&c.faces[f]).iter().map(|&v| c.positions[v as usize]).collect();
    polygon_normal(&pts).normalize_or_zero()
}

/// Middle of face `f`.
pub fn face_center(c: &Cage, f: usize) -> Vec3 {
    let k = corners(&c.faces[f]);
    k.iter().map(|&v| c.positions[v as usize]).sum::<Vec3>() / k.len().max(1) as f32
}

/// Grow from `start` across edges to faces that `keep` accepts.
fn flood(c: &Cage, adj: &Adjacency, start: usize, keep: impl Fn(usize) -> bool) -> Vec<usize> {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut out = Vec::new();
    while let Some(f) = queue.pop_front() {
        out.push(f);
        for g in adj.neighbours(c, f) {
            if keep(g) && seen.insert(g) {
                queue.push_back(g);
            }
        }
    }
    out.sort_unstable();
    out
}

/// The faces `reach` covers from `face`. `edge` is the edge of `face`
/// nearest the pointer, which picks the direction of a loop.
pub fn reach_faces(c: &Cage, face: usize, edge: Option<[u32; 2]>, reach: Reach) -> Vec<usize> {
    if face >= c.faces.len() {
        return Vec::new();
    }
    let adj = Adjacency::new(c);
    let group = c.faces[face].group;
    match reach {
        Reach::One => vec![face],
        Reach::Loop => face_loop(c, &adj, face, edge),
        Reach::Group => (0..c.faces.len()).filter(|&f| c.faces[f].group == group).collect(),
        Reach::GroupIsland => flood(c, &adj, face, |g| c.faces[g].group == group),
        Reach::Flat => {
            let n = face_normal(c, face);
            let cos = FLAT_DEGREES.to_radians().cos();
            flood(c, &adj, face, |g| face_normal(c, g).dot(n) >= cos)
        }
        Reach::Island => flood(c, &adj, face, |_| true),
    }
}

/// The quad loop through `face`, running across `edge` (and the opposite
/// edge) both ways until it meets a non-quad, a border or itself. A
/// non-quad face is its own loop.
fn face_loop(c: &Cage, adj: &Adjacency, face: usize, edge: Option<[u32; 2]>) -> Vec<usize> {
    let quad = |f: usize| c.faces[f].len() == 4;
    if !quad(face) {
        return vec![face];
    }
    let k = corners(&c.faces[face]).to_vec();
    let side = edge.and_then(|e| (0..4).find(|&i| edge_key(k[i], k[(i + 1) % 4]) == edge_key(e[0], e[1]))).unwrap_or(0);
    let mut out = HashSet::from([face]);
    // Leave through edge `side`, then through the opposite edge.
    for start in [side, (side + 2) % 4] {
        let (mut f, mut e) = (face, edge_key(k[start], k[(start + 1) % 4]));
        while let Some(&g) = adj.faces_of_edge(e[0], e[1]).iter().find(|&&g| g != f) {
            if !quad(g) || !out.insert(g) {
                break;
            }
            let q = corners(&c.faces[g]);
            let Some(i) = (0..4).find(|&i| edge_key(q[i], q[(i + 1) % 4]) == e) else { break };
            e = edge_key(q[(i + 2) % 4], q[(i + 3) % 4]);
            f = g;
        }
    }
    let mut v: Vec<usize> = out.into_iter().collect();
    v.sort_unstable();
    v
}

/// The faces of `faces` mirrored across the plane `x = cx` (matched by
/// their mirrored middles, within `tol`), together with `faces` themselves.
pub fn with_mirror(c: &Cage, faces: &[usize], cx: f32, tol: f32) -> Vec<usize> {
    let reflect = |p: Vec3| Vec3::new(2.0 * cx - p.x, p.y, p.z);
    let mut out: HashSet<usize> = faces.iter().copied().collect();
    let centers: Vec<Vec3> = (0..c.faces.len()).map(|f| face_center(c, f)).collect();
    for &f in faces {
        let want = reflect(centers[f]);
        if let Some(g) = (0..c.faces.len())
            .filter(|&g| c.faces[g].len() == c.faces[f].len())
            .min_by(|&a, &b| centers[a].distance_squared(want).total_cmp(&centers[b].distance_squared(want)))
            .filter(|&g| centers[g].distance(want) <= tol)
        {
            out.insert(g);
        }
    }
    let mut v: Vec<usize> = out.into_iter().collect();
    v.sort_unstable();
    v
}

/// Points moving during a drag: each from where it started, along its own
/// direction, by the dragged amount.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slide {
    pub verts: Vec<u32>,
    pub start: Vec<Vec3>,
    pub dir: Vec<Vec3>,
    /// Directions for a negative amount, when they differ (edge slide goes
    /// towards one side or the other); empty means `-dir`.
    pub back: Vec<Vec3>,
}

impl Slide {
    pub(crate) fn push(&mut self, c: &Cage, v: u32, dir: Vec3) {
        self.verts.push(v);
        self.start.push(c.positions[v as usize]);
        self.dir.push(dir);
    }

    /// Place every point at `start + dir * amount` (`start + back * -amount`
    /// for a negative amount, when `back` is set).
    pub fn apply(&self, c: &mut Cage, amount: f32) {
        for (i, ((&v, &s), &d)) in self.verts.iter().zip(&self.start).zip(&self.dir).enumerate() {
            c.positions[v as usize] = match self.back.get(i) {
                Some(&b) if amount < 0.0 => s - b * amount,
                _ => s + d * amount,
            };
        }
    }

    /// Add `other`'s points (both from one edit, numbered in the same cage).
    pub fn extend(&mut self, other: Slide) {
        let had_back = !self.back.is_empty() || !other.back.is_empty();
        if had_back {
            let pad = |s: &Slide| if s.back.is_empty() { s.dir.iter().map(|d| -*d).collect() } else { s.back.clone() };
            let mut back = pad(self);
            back.extend(pad(&other));
            self.back = back;
        }
        self.verts.extend(other.verts);
        self.start.extend(other.start);
        self.dir.extend(other.dir);
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }
}

/// A free face group number: one above the highest in use.
pub fn new_group(c: &Cage) -> u32 {
    c.faces.iter().map(|f| f.group).max().map_or(1, |g| g + 1)
}

/// Directed edges (corner order) of the faces in `region` that no other
/// face of `region` shares: the region's outline.
fn region_border(c: &Cage, region: &HashSet<usize>, adj: &Adjacency) -> Vec<(u32, u32, usize)> {
    let mut out = Vec::new();
    for &f in region {
        let k = corners(&c.faces[f]);
        for i in 0..k.len() {
            let (a, b) = (k[i], k[(i + 1) % k.len()]);
            if !adj.faces_of_edge(a, b).iter().any(|&g| g != f && region.contains(&g)) {
                out.push((a, b, f));
            }
        }
    }
    out.sort_unstable();
    out
}

/// Normal at each point of `region`, from the region's faces only, so a
/// region moves out as one block.
fn region_point_normals(c: &Cage, region: &HashSet<usize>) -> HashMap<u32, Vec3> {
    let mut acc: HashMap<u32, Vec3> = HashMap::new();
    for &f in region {
        let n = face_normal(c, f);
        for &v in corners(&c.faces[f]) {
            *acc.entry(v).or_default() += n;
        }
    }
    acc.into_iter().map(|(v, n)| (v, n.normalize_or_zero())).collect()
}

/// Cut `region` loose along its outline: each outline point gets a copy
/// that the region's faces use instead, and a quad joins every outline
/// edge to its copy (the walls of an extrude, the rim of an inset). The
/// walls get face group `wall_group`. Returns the copies (original ->
/// copy) and the new wall faces.
fn split_region(c: &mut Cage, region: &HashSet<usize>, wall_group: u32) -> (HashMap<u32, u32>, Vec<usize>) {
    let adj = Adjacency::new(c);
    let border = region_border(c, region, &adj);
    let mut copy: HashMap<u32, u32> = HashMap::new();
    let mut outline: Vec<u32> = border.iter().flat_map(|&(a, b, _)| [a, b]).collect();
    outline.sort_unstable();
    outline.dedup();
    for v in outline {
        let p = c.positions[v as usize];
        copy.insert(v, c.add_vertex(p));
    }
    for &f in region {
        for v in c.faces[f].corners.iter_mut() {
            if let Some(&n) = copy.get(v) {
                *v = n;
            }
        }
    }
    let mut walls = Vec::new();
    for &(a, b, _) in &border {
        // The region's face ran a -> b; the face outside runs b -> a, the
        // moved region face runs a' -> b'. The wall a, b, b', a' matches both.
        c.faces.push(Face::with_group(vec![a, b, copy[&b], copy[&a]], wall_group));
        walls.push(c.faces.len() - 1);
    }
    (copy, walls)
}

/// Extrude `faces`: walls go up around the outline and the faces become
/// the cap. With `each` set every face is extruded on its own; otherwise
/// touching faces move out together as one block. Returns the points to
/// slide (out along the region normals) and the faces made. The caps get
/// a new face group and the walls another.
pub fn extrude(c: &mut Cage, faces: &[usize], each: bool) -> (Slide, Vec<usize>) {
    let regions: Vec<HashSet<usize>> =
        if each { faces.iter().map(|&f| HashSet::from([f])).collect() } else { vec![faces.iter().copied().collect()] };
    let cap_group = new_group(c);
    let wall_group = cap_group + 1;
    let mut slide = Slide::default();
    let mut made = Vec::new();
    let mut moved = HashSet::new();
    for region in regions {
        if region.iter().any(|&f| f >= c.faces.len()) {
            continue;
        }
        let normals = region_point_normals(c, &region);
        let (copy, walls) = split_region(c, &region, wall_group);
        made.extend(walls);
        for &f in &region {
            c.faces[f].group = cap_group;
            for &v in &c.faces[f].corners.clone() {
                if moved.insert(v) {
                    // A copy moves along its original's normal.
                    let orig = copy.iter().find(|(_, n)| **n == v).map_or(v, |(o, _)| *o);
                    let n = normals.get(&orig).or_else(|| normals.get(&v)).copied().unwrap_or(Vec3::ZERO);
                    slide.push(c, v, n);
                }
            }
        }
    }
    (slide, made)
}

/// Inset `faces`: a rim of quads goes in around the outline and the faces
/// shrink inside it. With `each` set every face gets its own rim;
/// otherwise touching faces are inset as one region. Returns the points to
/// slide (inward, scaled so the rim's width is the slid amount) and the
/// rim faces. The rim gets a new face group.
pub fn inset(c: &mut Cage, faces: &[usize], each: bool) -> (Slide, Vec<usize>) {
    let regions: Vec<HashSet<usize>> =
        if each { faces.iter().map(|&f| HashSet::from([f])).collect() } else { vec![faces.iter().copied().collect()] };
    let rim_group = new_group(c);
    let mut slide = Slide::default();
    let mut made = Vec::new();
    for region in regions {
        if region.iter().any(|&f| f >= c.faces.len()) {
            continue;
        }
        // Inward direction of each outline edge, in its face's plane.
        let adj = Adjacency::new(c);
        let border = region_border(c, &region, &adj);
        let mut inward: HashMap<u32, Vec<Vec3>> = HashMap::new();
        for &(a, b, f) in &border {
            let n = face_normal(c, f);
            let e = (c.positions[b as usize] - c.positions[a as usize]).normalize_or_zero();
            let perp = n.cross(e).normalize_or_zero();
            inward.entry(a).or_default().push(perp);
            inward.entry(b).or_default().push(perp);
        }
        let (copy, walls) = split_region(c, &region, rim_group);
        made.extend(walls);
        let mut order: Vec<(u32, u32)> = copy.into_iter().collect();
        order.sort_unstable();
        for (orig, new) in order {
            let perps = &inward[&orig];
            let sum: Vec3 = perps.iter().copied().sum();
            let m = sum.normalize_or_zero();
            // Mitre: keep the rim the same width along both edges.
            let cos = perps.iter().map(|p| p.dot(m)).fold(1.0f32, f32::min).max(0.25);
            slide.push(c, new, m / cos);
        }
    }
    (slide, made)
}

/// Move the points of `faces` along their normals (free of the rest of
/// the mesh, which stretches to follow). Returns the points to slide.
pub fn move_faces(c: &Cage, faces: &[usize]) -> Slide {
    let region: HashSet<usize> = faces.iter().copied().filter(|&f| f < c.faces.len()).collect();
    let normals = region_point_normals(c, &region);
    let mut verts: Vec<u32> = normals.keys().copied().collect();
    verts.sort_unstable();
    let mut slide = Slide::default();
    for v in verts {
        slide.push(c, v, normals[&v]);
    }
    slide
}

/// Delete `faces`, leaving a hole, and drop points no face uses any more.
pub fn delete_faces(c: &mut Cage, faces: &[usize]) {
    let gone: HashSet<usize> = faces.iter().copied().collect();
    let mut i = 0;
    c.faces.retain(|_| {
        let keep = !gone.contains(&i);
        i += 1;
        keep
    });
    remove_unused_points(c);
}

/// Drop points no face uses, renumbering the rest (in order).
pub fn remove_unused_points(c: &mut Cage) {
    let used = c.used();
    if used.iter().all(|u| *u) {
        return;
    }
    let mut map = vec![u32::MAX; c.positions.len()];
    let mut positions = Vec::new();
    let mut pinned = Vec::new();
    for (v, &u) in used.iter().enumerate() {
        if u {
            map[v] = positions.len() as u32;
            positions.push(c.positions[v]);
            pinned.push(c.is_pinned(v as u32));
        }
    }
    for f in &mut c.faces {
        for v in f.corners.iter_mut() {
            *v = map[*v as usize];
        }
    }
    c.positions = positions;
    c.pinned = if pinned.iter().any(|p| *p) { pinned } else { Vec::new() };
    c.remap_creases(|v| map.get(v as usize).copied().filter(|&m| m != u32::MAX));
}

/// How far an edge action spreads from the edge it's applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeReach {
    /// Just that edge.
    One,
    /// The edge loop through it: on through points with four edges, until
    /// a pole or a border. On a border, the whole border loop.
    Loop,
    /// The edges across its quad ring (each quad's opposite edge).
    Ring,
}

impl EdgeReach {
    pub const ALL: [EdgeReach; 3] = [EdgeReach::One, EdgeReach::Loop, EdgeReach::Ring];

    pub fn label(self) -> &'static str {
        match self {
            EdgeReach::One => "This edge",
            EdgeReach::Loop => "Edge loop",
            EdgeReach::Ring => "Edge ring",
        }
    }
}

/// The edges at each point, as the other end.
fn point_edges(c: &Cage) -> HashMap<u32, Vec<u32>> {
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    for [a, b] in c.edges() {
        out.entry(a).or_default().push(b);
        out.entry(b).or_default().push(a);
    }
    out
}

/// The edges `reach` covers from edge `(a, b)`, as `[low, high]` keys.
pub fn reach_edges(c: &Cage, a: u32, b: u32, reach: EdgeReach) -> Vec<[u32; 2]> {
    let adj = Adjacency::new(c);
    if adj.faces_of_edge(a, b).is_empty() {
        return Vec::new();
    }
    let mut out = match reach {
        EdgeReach::One => vec![edge_key(a, b)],
        EdgeReach::Loop if adj.faces_of_edge(a, b).len() == 1 => border_loop(c, &adj, a, b),
        EdgeReach::Loop => edge_loop(c, &adj, a, b),
        EdgeReach::Ring => edge_ring(c, &adj, a, b),
    };
    out.sort_unstable();
    out.dedup();
    out
}

/// The edge loop through `(a, b)`: at each end, through a point with
/// exactly four edges, carry on along the edge that shares no face with
/// the one we came from.
fn edge_loop(c: &Cage, adj: &Adjacency, a: u32, b: u32) -> Vec<[u32; 2]> {
    let pe = point_edges(c);
    let mut out = HashSet::from([edge_key(a, b)]);
    for (mut from, mut at) in [(a, b), (b, a)] {
        while let Some(next) = pe.get(&at) {
            if next.len() != 4 {
                break;
            }
            let came: HashSet<usize> = adj.faces_of_edge(from, at).iter().copied().collect();
            if came.len() != 2 {
                break;
            }
            let Some(&w) =
                next.iter().find(|&&w| w != from && !adj.faces_of_edge(at, w).iter().any(|f| came.contains(f)))
            else {
                break;
            };
            if !out.insert(edge_key(at, w)) {
                break;
            }
            from = at;
            at = w;
        }
    }
    out.into_iter().collect()
}

/// The border edges around the hole or open edge that `(a, b)` is on.
fn border_loop(c: &Cage, adj: &Adjacency, a: u32, b: u32) -> Vec<[u32; 2]> {
    let border: Vec<[u32; 2]> = c.border_edges();
    let mut at_point: HashMap<u32, Vec<u32>> = HashMap::new();
    for [x, y] in &border {
        at_point.entry(*x).or_default().push(*y);
        at_point.entry(*y).or_default().push(*x);
    }
    let _ = adj;
    let mut out = HashSet::from([edge_key(a, b)]);
    let mut queue = vec![a, b];
    while let Some(v) = queue.pop() {
        for &w in at_point.get(&v).into_iter().flatten() {
            if out.insert(edge_key(v, w)) {
                queue.push(w);
            }
        }
    }
    out.into_iter().collect()
}

/// The edges across the quad ring through `(a, b)`.
fn edge_ring(c: &Cage, adj: &Adjacency, a: u32, b: u32) -> Vec<[u32; 2]> {
    let mut out = HashSet::from([edge_key(a, b)]);
    let mut seen: HashSet<usize> = HashSet::new();
    for &first in adj.faces_of_edge(a, b) {
        let (mut e, mut f) = (edge_key(a, b), first);
        loop {
            if c.faces[f].len() != 4 || !seen.insert(f) {
                break;
            }
            let q = corners(&c.faces[f]);
            let Some(i) = (0..4).find(|&i| edge_key(q[i], q[(i + 1) % 4]) == e) else { break };
            e = edge_key(q[(i + 2) % 4], q[(i + 3) % 4]);
            out.insert(e);
            match adj.faces_of_edge(e[0], e[1]).iter().find(|&&g| g != f) {
                Some(&g) => f = g,
                None => break,
            }
        }
    }
    out.into_iter().collect()
}

/// The point at the mirror image of point `v` across `x = cx`, if any.
pub fn mirror_point(c: &Cage, v: u32, cx: f32, tol: f32) -> Option<u32> {
    let p = c.positions[v as usize];
    let want = Vec3::new(2.0 * cx - p.x, p.y, p.z);
    let used = c.used();
    (0..c.positions.len() as u32)
        .filter(|&w| used[w as usize])
        .min_by(|&x, &y| {
            c.positions[x as usize].distance_squared(want).total_cmp(&c.positions[y as usize].distance_squared(want))
        })
        .filter(|&w| c.positions[w as usize].distance(want) <= tol)
}

/// `edges` together with their mirror images across `x = cx`.
pub fn edges_with_mirror(c: &Cage, edges: &[[u32; 2]], cx: f32, tol: f32) -> Vec<[u32; 2]> {
    let ek: HashSet<[u32; 2]> = c.edges().into_iter().collect();
    let mut out: HashSet<[u32; 2]> = edges.iter().copied().collect();
    for &[a, b] in edges {
        if let (Some(x), Some(y)) = (mirror_point(c, a, cx, tol), mirror_point(c, b, cx, tol))
            && ek.contains(&edge_key(x, y))
        {
            out.insert(edge_key(x, y));
        }
    }
    let mut v: Vec<[u32; 2]> = out.into_iter().collect();
    v.sort_unstable();
    v
}

/// Insert an edge loop across the quad ring through `(a, b)` at `t`.
/// Returns the points to slide: sliding by `s` puts the loop at `s`
/// along the ring (0 at `a`'s side, 1 at `b`'s).
pub fn insert_loop(c: &mut Cage, a: u32, b: u32, t: f32) -> Slide {
    let made = c.insert_loop_ends(a, b, t);
    let mut slide = Slide::default();
    for (m, from, to) in made {
        let p = c.positions[from as usize];
        slide.verts.push(m);
        slide.start.push(p);
        slide.dir.push(c.positions[to as usize] - p);
    }
    slide
}

/// Remove `edges`, merging the two faces on each into one (a polygon
/// with more corners). Border edges, and edges whose two sides are the
/// same face, are left alone. Points left in the middle of a straight
/// run of a single face (two edges, nothing else) are dropped. Returns how
/// many edges were removed.
pub fn dissolve_edges(c: &mut Cage, edges: &[[u32; 2]]) -> usize {
    let mut removed = 0;
    for &[a, b] in edges {
        let adj = Adjacency::new(c);
        let fs = adj.faces_of_edge(a, b).to_vec();
        if fs.len() != 2 || fs[0] == fs[1] {
            continue;
        }
        // The face running a -> b and the one running b -> a.
        let runs = |f: usize, x: u32, y: u32| {
            let k = corners(&c.faces[f]);
            (0..k.len()).any(|i| k[i] == x && k[(i + 1) % k.len()] == y)
        };
        let (f, g) = if runs(fs[0], a, b) { (fs[0], fs[1]) } else { (fs[1], fs[0]) };
        if !runs(f, a, b) || !runs(g, b, a) {
            continue;
        }
        // f from b round to a, then g from a round to b without its ends.
        let walk = |face: usize, start: u32, end: u32| -> Vec<u32> {
            let k = corners(&c.faces[face]);
            let i = k.iter().position(|&v| v == start).unwrap();
            let mut out = Vec::new();
            for j in 0..k.len() {
                let v = k[(i + j) % k.len()];
                out.push(v);
                if v == end {
                    break;
                }
            }
            out
        };
        let mut merged = walk(f, b, a);
        let inner = walk(g, a, b);
        merged.extend(&inner[1..inner.len() - 1]);
        // Two faces sharing more than this edge would repeat a corner.
        let distinct: HashSet<u32> = merged.iter().copied().collect();
        if distinct.len() != merged.len() || merged.len() < 3 {
            continue;
        }
        let group = c.faces[f].group;
        c.faces[f] = Face::with_group(merged, group);
        c.faces.remove(g);
        removed += 1;
    }
    drop_straight_points(c);
    remove_unused_points(c);
    removed
}

/// Drop corners that only two edges meet at, both bordering the same
/// faces (left over after removing a loop): the faces just lose a corner.
fn drop_straight_points(c: &mut Cage) {
    let pe = point_edges(c);
    let border: HashSet<u32> = c.border_edges().into_iter().flatten().collect();
    let drop: HashSet<u32> = pe.iter().filter(|(v, e)| e.len() == 2 && !border.contains(v)).map(|(v, _)| *v).collect();
    if drop.is_empty() {
        return;
    }
    for f in &mut c.faces {
        if f.corners.iter().filter(|v| !drop.contains(v)).count() >= 3 {
            f.corners.retain(|v| !drop.contains(v));
        }
    }
}

/// Pull new faces out of border `edges`: each gets a quad whose outer
/// edge starts on it. Touching border edges share their new points.
/// Returns the points to slide (outward, across the edge in the plane of
/// its face) and the new faces, which get a new face group.
pub fn extrude_border(c: &mut Cage, edges: &[[u32; 2]]) -> (Slide, Vec<usize>) {
    let adj = Adjacency::new(c);
    // Each border edge in the direction its face runs it.
    let mut directed = Vec::new();
    for &[a, b] in edges {
        let fs = adj.faces_of_edge(a, b);
        if fs.len() != 1 {
            continue;
        }
        let k = corners(&c.faces[fs[0]]);
        let forward = (0..k.len()).any(|i| k[i] == a && k[(i + 1) % k.len()] == b);
        directed.push(if forward { (a, b, fs[0]) } else { (b, a, fs[0]) });
    }
    let group = new_group(c);
    let mut out_dir: HashMap<u32, Vec3> = HashMap::new();
    for &(a, b, f) in &directed {
        let n = face_normal(c, f);
        let e = c.positions[b as usize] - c.positions[a as usize];
        let out = e.cross(n).normalize_or_zero();
        *out_dir.entry(a).or_default() += out;
        *out_dir.entry(b).or_default() += out;
    }
    let mut copy: HashMap<u32, u32> = HashMap::new();
    let mut points: Vec<u32> = out_dir.keys().copied().collect();
    points.sort_unstable();
    let mut slide = Slide::default();
    for v in points {
        let p = c.positions[v as usize];
        let n = c.add_vertex(p);
        copy.insert(v, n);
        slide.push(c, n, out_dir[&v].normalize_or_zero());
    }
    let mut made = Vec::new();
    for (a, b, _) in directed {
        // The face runs a -> b; the new quad runs b -> a on it.
        c.faces.push(Face::with_group(vec![b, a, copy[&a], copy[&b]], group));
        made.push(c.faces.len() - 1);
    }
    (slide, made)
}

/// Close the hole whose border `(a, b)` is on with one face (a polygon
/// with as many corners as the hole has points), in a new face group.
/// Returns the new face, or None when `(a, b)` isn't on a closed border.
pub fn close_hole(c: &mut Cage, a: u32, b: u32) -> Option<usize> {
    let adj = Adjacency::new(c);
    let fs = adj.faces_of_edge(a, b);
    if fs.len() != 1 {
        return None;
    }
    // Next point along the border, in the direction faces run it.
    let mut next: HashMap<u32, u32> = HashMap::new();
    for [x, y] in c.border_edges() {
        let f = adj.faces_of_edge(x, y)[0];
        let k = corners(&c.faces[f]);
        let forward = (0..k.len()).any(|i| k[i] == x && k[(i + 1) % k.len()] == y);
        let (p, q) = if forward { (x, y) } else { (y, x) };
        if next.insert(p, q).is_some() {
            return None; // two border runs through one point: ambiguous
        }
    }
    let start = a;
    let mut ring = vec![start];
    let mut at = *next.get(&start)?;
    while at != start {
        if ring.len() > next.len() {
            return None;
        }
        ring.push(at);
        at = *next.get(&at)?;
    }
    if ring.len() < 3 {
        return None;
    }
    // The faces run the border one way; the new face runs it the other.
    ring.reverse();
    let group = new_group(c);
    c.faces.push(Face::with_group(ring, group));
    Some(c.faces.len() - 1)
}

/// Delete `points` and the faces around them, leaving a hole.
pub fn delete_points(c: &mut Cage, points: &[u32]) {
    let gone: HashSet<u32> = points.iter().copied().collect();
    c.faces.retain(|f| !f.corners.iter().any(|v| gone.contains(v)));
    remove_unused_points(c);
}

/// A cage for `mesh` as it is, linked to it (no rebuild, so its mask and
/// vertex color stay): triangle pairs the mesh counts as quads become
/// quads, the rest stay triangles, and each face takes its triangle's
/// face group. Cage vertex `i` is mesh vertex `i`.
pub fn cage_from_mesh(mesh: &crate::Mesh) -> Cage {
    let tris = &mesh.triangles;
    let groups = mesh.groups();
    let diagonals = mesh.quad_diagonals_ref();
    let mut by_edge: HashMap<(u32, u32), usize> = HashMap::with_capacity(tris.len() * 3);
    for (t, tri) in tris.iter().enumerate() {
        for i in 0..3 {
            by_edge.insert((tri[i], tri[(i + 1) % 3]), t);
        }
    }
    let mut used = vec![false; tris.len()];
    let mut faces = Vec::with_capacity(tris.len());
    for t in 0..tris.len() {
        if used[t] {
            continue;
        }
        used[t] = true;
        let tri = tris[t];
        let group = groups.get(t).copied().unwrap_or(0);
        // Rotate so a diagonal, if any, runs from corner 2 back to corner 0:
        // the quad is then [p0, p1, p2, w], split along p0-p2 as the cage does.
        let paired = (0..3).find_map(|r| {
            let (p0, p1, p2) = (tri[r], tri[(r + 1) % 3], tri[(r + 2) % 3]);
            if !diagonals.contains(&edge_key(p2, p0)) {
                return None;
            }
            let &u = by_edge.get(&(p0, p2))?;
            if used[u] {
                return None;
            }
            let w = *tris[u].iter().find(|&&x| x != p0 && x != p2)?;
            Some((u, vec![p0, p1, p2, w]))
        });
        match paired {
            Some((u, quad)) => {
                used[u] = true;
                faces.push(Face::with_group(quad, group));
            }
            None => faces.push(Face::with_group(tri.to_vec(), group)),
        }
    }
    Cage { positions: mesh.positions.clone(), faces, linked: Some(mesh.topology_version()), ..Default::default() }
}

/// Check the cage's INVARIANT (see the module docs). Returns what's wrong.
pub fn validate(c: &Cage) -> Result<(), String> {
    let n = c.positions.len();
    let mut directed: HashSet<(u32, u32)> = HashSet::new();
    let mut count: HashMap<[u32; 2], usize> = HashMap::new();
    for (fi, f) in c.faces.iter().enumerate() {
        let k = corners(f);
        if k.len() < 3 {
            return Err(format!("face {fi} has {} corners", k.len()));
        }
        if k.iter().any(|&v| v as usize >= n) {
            return Err(format!("face {fi} uses a missing point"));
        }
        let distinct: HashSet<u32> = k.iter().copied().collect();
        if distinct.len() != k.len() {
            return Err(format!("face {fi} repeats a corner"));
        }
        for i in 0..k.len() {
            let (a, b) = (k[i], k[(i + 1) % k.len()]);
            if !directed.insert((a, b)) {
                return Err(format!("edge {a}-{b} runs the same way in two faces (face {fi})"));
            }
            let e = count.entry(edge_key(a, b)).or_default();
            *e += 1;
            if *e > 2 {
                return Err(format!("edge {a}-{b} has more than two faces"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed cube of six quads, faces pointing out, each its own group.
    fn cube() -> Cage {
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let positions = vec![
            p(-1., -1., -1.),
            p(1., -1., -1.),
            p(1., 1., -1.),
            p(-1., 1., -1.),
            p(-1., -1., 1.),
            p(1., -1., 1.),
            p(1., 1., 1.),
            p(-1., 1., 1.),
        ];
        let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]];
        let faces = quads.iter().enumerate().map(|(i, q)| Face::with_group(q.to_vec(), i as u32)).collect();
        Cage { positions, faces, ..Default::default() }
    }

    /// An `n` × `n` grid of quads in the XY plane facing +Z.
    fn grid(n: u32) -> Cage {
        let mut c = Cage::default();
        for y in 0..=n {
            for x in 0..=n {
                c.positions.push(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        let at = |x: u32, y: u32| y * (n + 1) + x;
        for y in 0..n {
            for x in 0..n {
                c.faces.push(Face::new(vec![at(x, y), at(x + 1, y), at(x + 1, y + 1), at(x, y + 1)]));
            }
        }
        c
    }

    fn top(c: &Cage) -> usize {
        (0..c.faces.len()).max_by(|&a, &b| face_center(c, a).z.total_cmp(&face_center(c, b).z)).unwrap()
    }

    fn euler(c: &Cage) -> i64 {
        c.positions.len() as i64 - c.edges().len() as i64 + c.faces.len() as i64
    }

    #[test]
    fn extruding_one_face_of_a_cube_makes_ten_faces_and_stays_closed() {
        let mut c = cube();
        let f = top(&c);
        let (slide, made) = extrude(&mut c, &[f], false);
        validate(&c).unwrap();
        assert_eq!(c.faces.len(), 10);
        assert_eq!(c.positions.len(), 12);
        assert_eq!(made.len(), 4);
        assert_eq!(euler(&c), 2, "still a closed surface");
        assert!(c.border_edges().is_empty());
        slide.apply(&mut c, 1.5);
        assert!((face_center(&c, f).z - 2.5).abs() < 1e-5, "the cap moved out along +Z");
        // Groups: the cap and walls are new groups, the rest keep theirs.
        assert_eq!(c.faces[f].group, 6);
        assert!(made.iter().all(|&w| c.faces[w].group == 7));
        validate(&c).unwrap();
    }

    #[test]
    fn touching_faces_extrude_as_one_block_or_each_on_its_own() {
        let mut c = grid(3);
        let (slide, made) = extrude(&mut c, &[0, 1], false);
        validate(&c).unwrap();
        assert_eq!(made.len(), 6, "a 2×1 block has six outline edges");
        slide.apply(&mut c, 1.0);
        assert!(
            c.faces[0]
                .corners
                .iter()
                .chain(&c.faces[1].corners)
                .all(|&v| (c.positions[v as usize].z - 1.0).abs() < 1e-6)
        );
        let mut c = grid(3);
        let (_, made) = extrude(&mut c, &[0, 1], true);
        validate(&c).unwrap();
        assert_eq!(made.len(), 8, "each face gets four walls");
    }

    #[test]
    fn inset_keeps_the_rim_width_even_and_the_face_flat() {
        let mut c = grid(1);
        let (slide, made) = inset(&mut c, &[0], false);
        validate(&c).unwrap();
        assert_eq!(made.len(), 4);
        assert_eq!(c.faces.len(), 5);
        slide.apply(&mut c, 0.25);
        let inner: Vec<Vec3> = c.faces[0].corners.iter().map(|&v| c.positions[v as usize]).collect();
        for p in inner {
            assert!((p.x - 0.25).abs() < 1e-5 || (p.x - 0.75).abs() < 1e-5, "{p}");
            assert!((p.y - 0.25).abs() < 1e-5 || (p.y - 0.75).abs() < 1e-5, "{p}");
            assert!(p.z.abs() < 1e-6);
        }
        // A 2×2 region gets one rim around the outside, none between.
        let mut c = grid(2);
        let (_, made) = inset(&mut c, &[0, 1, 2, 3], false);
        validate(&c).unwrap();
        assert_eq!(made.len(), 8);
    }

    #[test]
    fn delete_leaves_a_hole_and_drops_loose_points() {
        let mut c = cube();
        let f = top(&c);
        delete_faces(&mut c, &[f]);
        validate(&c).unwrap();
        assert_eq!(c.faces.len(), 5);
        assert_eq!(c.border_edges().len(), 4);
        let mut g = grid(1);
        delete_faces(&mut g, &[0]);
        assert!(g.positions.is_empty() && g.faces.is_empty());
    }

    #[test]
    fn move_slides_points_along_the_region_normal() {
        let c0 = grid(2);
        let slide = move_faces(&c0, &[0]);
        let mut c = c0.clone();
        slide.apply(&mut c, 0.5);
        assert_eq!(slide.verts.len(), 4);
        for &v in &slide.verts {
            assert!((c.positions[v as usize].z - 0.5).abs() < 1e-6);
        }
        slide.apply(&mut c, 0.0);
        assert_eq!(c, c0, "sliding back to zero restores it exactly");
    }

    #[test]
    fn reach_loop_group_flat_and_island() {
        let c = grid(4);
        // Loop across the edge between corners 0 and 1 of face 5 (a row).
        let k = c.faces[5].corners.clone();
        let row = reach_faces(&c, 5, Some([k[1], k[2]]), Reach::Loop);
        assert_eq!(row, vec![4, 5, 6, 7]);
        let col = reach_faces(&c, 5, Some([k[0], k[1]]), Reach::Loop);
        assert_eq!(col, vec![1, 5, 9, 13]);
        assert_eq!(reach_faces(&c, 5, None, Reach::Island).len(), 16);
        assert_eq!(reach_faces(&c, 5, None, Reach::Flat).len(), 16);
        // A cube's flat region is one side; a cube's loop goes all round.
        let cube = cube();
        let f = top(&cube);
        assert_eq!(reach_faces(&cube, f, None, Reach::Flat), vec![f]);
        let kk = cube.faces[f].corners.clone();
        assert_eq!(reach_faces(&cube, f, Some([kk[0], kk[1]]), Reach::Loop).len(), 4);
        // Groups: two groups on a grid.
        let mut g = grid(2);
        g.faces[3].group = 1;
        g.faces[0].group = 1;
        assert_eq!(reach_faces(&g, 0, None, Reach::Group), vec![0, 3]);
        assert_eq!(reach_faces(&g, 0, None, Reach::GroupIsland), vec![0], "0 and 3 only touch at a corner");
        assert_eq!(reach_faces(&g, 1, None, Reach::GroupIsland), vec![1], "1 and 2 also only touch at a corner");
    }

    #[test]
    fn a_mesh_becomes_a_linked_cage_of_its_quads() {
        let mut g = grid(3);
        g.faces[4].group = 2;
        let mesh = g.build_mesh().unwrap();
        let c = cage_from_mesh(&mesh);
        validate(&c).unwrap();
        assert!(c.is_linked_to(&mesh));
        assert_eq!((c.quad_count(), c.triangle_count()), (9, 0));
        assert_eq!(c.faces.iter().filter(|f| f.group == 2).count(), 1);
        // Same surface: its triangles are the mesh's.
        let (tris, _) = c.triangulate();
        let key = |t: &[u32; 3]| {
            let mut k = *t;
            let m = (0..3).min_by_key(|&i| k[i]).unwrap();
            k.rotate_left(m);
            k
        };
        let mut a: Vec<[u32; 3]> = tris.iter().map(key).collect();
        let mut b: Vec<[u32; 3]> = mesh.triangles.iter().map(key).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
        // A primitive cube: six quads.
        let cube = cage_from_mesh(&crate::primitives::cube(1, 1.0));
        validate(&cube).unwrap();
        assert_eq!(cube.quad_count(), 6);
    }

    #[test]
    fn edge_loops_rings_and_border_loops() {
        let c = grid(3);
        // Interior vertical edge between faces 3 and 4: points (1,1)-(1,2).
        let at = |x: u32, y: u32| y * 4 + x;
        let lp = reach_edges(&c, at(1, 1), at(1, 2), EdgeReach::Loop);
        assert_eq!(lp, vec![[at(1, 0), at(1, 1)], [at(1, 1), at(1, 2)], [at(1, 2), at(1, 3)]]);
        let ring = reach_edges(&c, at(1, 1), at(1, 2), EdgeReach::Ring);
        assert_eq!(ring.len(), 4, "{ring:?}");
        // On the border: the whole outline (12 edges).
        assert_eq!(reach_edges(&c, at(0, 0), at(1, 0), EdgeReach::Loop).len(), 12);
        // A cube's corners have three edges: a loop stops there, a ring goes round.
        let cube = cube();
        assert_eq!(reach_edges(&cube, 0, 1, EdgeReach::Loop).len(), 1);
        assert_eq!(reach_edges(&cube, 0, 1, EdgeReach::Ring).len(), 4);
    }

    #[test]
    fn insert_loop_slides_along_the_ring() {
        let mut c = grid(2);
        let at = |x: u32, y: u32| y * 3 + x;
        let slide = insert_loop(&mut c, at(0, 0), at(1, 0), 0.5);
        validate(&c).unwrap();
        assert_eq!(slide.verts.len(), 3);
        assert_eq!(c.faces.len(), 6);
        slide.apply(&mut c, 0.25);
        for &v in &slide.verts {
            assert!((c.positions[v as usize].x - 0.25).abs() < 1e-5, "{}", c.positions[v as usize]);
        }
    }

    #[test]
    fn dissolving_a_loop_merges_its_faces() {
        let mut c = grid(2);
        let at = |x: u32, y: u32| y * 3 + x;
        let lp = reach_edges(&c, at(1, 0), at(1, 1), EdgeReach::Loop);
        assert_eq!(lp.len(), 2);
        assert_eq!(dissolve_edges(&mut c, &lp), 2);
        validate(&c).unwrap();
        assert_eq!(c.faces.len(), 2);
        // The middle point (1,1) had only two edges left and is gone; the
        // border points (1,0), (1,2) stay as corners.
        assert_eq!(c.positions.len(), 8);
        assert!(c.faces.iter().all(|f| f.len() == 5), "{:?}", c.faces);
        // A border edge isn't removed.
        let mut g = grid(1);
        assert_eq!(dissolve_edges(&mut g, &[[0, 1]]), 0);
    }

    #[test]
    fn border_edges_extrude_outward_and_holes_close() {
        let mut c = grid(1);
        let (slide, made) = extrude_border(&mut c, &[[0, 1]]);
        validate(&c).unwrap();
        assert_eq!(made.len(), 1);
        slide.apply(&mut c, 1.0);
        for &v in &slide.verts {
            assert!((c.positions[v as usize].y + 1.0).abs() < 1e-5, "outward is -Y: {}", c.positions[v as usize]);
        }
        // The whole outline at once shares corner points.
        let mut c = grid(1);
        let all = reach_edges(&c, 0, 1, EdgeReach::Loop);
        let (slide, made) = extrude_border(&mut c, &all);
        validate(&c).unwrap();
        assert_eq!((made.len(), slide.verts.len()), (4, 4));
        // Delete a cube face and close the hole again.
        let mut cube = cube();
        let f = top(&cube);
        delete_faces(&mut cube, &[f]);
        let [a, b] = cube.border_edges()[0];
        let hole = close_hole(&mut cube, a, b).unwrap();
        validate(&cube).unwrap();
        assert!(cube.border_edges().is_empty());
        assert!(face_normal(&cube, hole).z > 0.9, "faces out");
    }

    #[test]
    fn deleting_a_point_removes_its_faces() {
        let mut c = grid(2);
        delete_points(&mut c, &[4]);
        validate(&c).unwrap();
        assert!(c.faces.is_empty() && c.positions.is_empty(), "the middle point touches all four faces");
        let mut c = grid(2);
        delete_points(&mut c, &[0]);
        assert_eq!(c.faces.len(), 3);
        assert_eq!(mirror_point(&grid(2), 0, 1.0, 1e-4), Some(2));
    }

    #[test]
    fn mirror_adds_the_matching_faces() {
        // A 2×2 grid centred on x = 1: face 0 (x 0..1) mirrors to face 1.
        let c = grid(2);
        assert_eq!(with_mirror(&c, &[0], 1.0, 1e-4), vec![0, 1]);
        assert_eq!(with_mirror(&c, &[2], 1.0, 1e-4), vec![2, 3]);
    }

    #[test]
    fn ngons_triangulate_into_the_built_mesh_with_their_groups() {
        // An L-shaped hexagon (concave) and a quad.
        let mut c = Cage::default();
        for (x, y) in [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.)] {
            c.positions.push(Vec3::new(x, y, 0.0));
        }
        c.faces.push(Face::with_group(vec![0, 1, 2, 3, 4, 5], 3));
        validate(&c).unwrap();
        let (tris, _, groups) = c.triangulate_with_groups();
        assert_eq!(tris.len(), 4);
        assert!(groups.iter().all(|&g| g == 3));
        // Ear clipping covers the L's area exactly (3), facing +Z.
        let area: f32 = tris
            .iter()
            .map(|t| {
                let [a, b, cc] = t.map(|i| c.positions[i as usize]);
                (b - a).cross(cc - a).z * 0.5
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-5, "{area}");
        let mesh = c.build_mesh().unwrap();
        assert_eq!(mesh.triangle_count(), 4);
        assert!(mesh.groups().iter().all(|&g| g == 3));
    }
}
