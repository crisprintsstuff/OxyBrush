//! Knife (issue #370, RFC 0033): cut a polygon cage along a path drawn on
//! it. The path is a list of clicked points on the surface, each snapped to
//! a corner, an edge or the inside of a face. Between two clicks the cut
//! follows the surface where it meets the plane through the eye and both
//! points (what lies under the line drawn on screen), face to face; it adds
//! a point on every edge it crosses and splits every face it passes through
//! along its piece of the path. Points clicked inside a face become corners
//! of the cut. A path end clicked inside a face carries on to that face's
//! edge, since a polygon can't hold a loose edge.
//!
//! [`knife_plan`] works the path out without changing anything (for the
//! preview); [`knife_cut`] applies a plan.

use crate::polyedit::face_normal;
use crate::retopo::{Cage, Face, corners, edge_key};
use glam::{DVec2, Vec3};
use std::collections::HashMap;

/// Where a clicked point sits on the cage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnifeSnap {
    /// On a corner.
    Point(u32),
    /// On edge `[low, high]`, `t` of the way from `low` to `high`.
    Edge([u32; 2], f32),
    /// Inside face `face`.
    Face(usize),
}

/// A clicked point: where it snapped and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnifePoint {
    pub snap: KnifeSnap,
    pub pos: Vec3,
}

/// The view the path was drawn in: cuts follow planes through the eye (or,
/// orthographic, along the view direction).
#[derive(Debug, Clone, Copy)]
pub struct KnifeView {
    pub eye: Vec3,
    pub forward: Vec3,
    pub orthographic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Node {
    Point(u32),
    Edge([u32; 2], f32),
    Inside(usize, Vec3),
}

/// A worked-out cut: the points it passes through, in order, each with the
/// face the path reaches it through (None for the first).
#[derive(Debug, Clone, Default)]
pub struct KnifePlan {
    nodes: Vec<(Node, Option<usize>)>,
    /// Whether the first and the last node were added past a clicked end
    /// inside a face, carrying the cut on to the face's edge.
    extended: (bool, bool),
}

impl KnifePlan {
    /// The cut as positions along the surface, for drawing.
    pub fn path(&self, c: &Cage) -> Vec<Vec3> {
        self.nodes.iter().map(|(n, _)| node_pos(c, *n)).collect()
    }

    /// Whether the cut carries on past the first and the last clicked point
    /// (each clicked inside a face) to the face's edge: the path's first
    /// and last pieces are those extensions.
    pub fn extended(&self) -> (bool, bool) {
        self.extended
    }

    /// How many edges the cut crosses or ends on.
    pub fn crossings(&self) -> usize {
        self.nodes.iter().filter(|(n, _)| matches!(n, Node::Edge(..))).count()
    }
}

fn node_pos(c: &Cage, n: Node) -> Vec3 {
    match n {
        Node::Point(v) => c.positions[v as usize],
        Node::Edge([a, b], t) => c.positions[a as usize].lerp(c.positions[b as usize], t),
        Node::Inside(_, p) => p,
    }
}

fn snap_node(p: &KnifePoint) -> Node {
    match p.snap {
        KnifeSnap::Point(v) => Node::Point(v),
        KnifeSnap::Edge([a, b], t) => {
            let ([lo, hi], t) = if a <= b { ([a, b], t) } else { ([b, a], 1.0 - t) };
            Node::Edge([lo, hi], t.clamp(0.0, 1.0))
        }
        KnifeSnap::Face(f) => Node::Inside(f, p.pos),
    }
}

/// Whether node `n` lies on face `f` (a corner, one of its edges, or inside it).
fn on_face(c: &Cage, f: usize, n: Node) -> bool {
    let k = corners(&c.faces[f]);
    match n {
        Node::Point(v) => k.contains(&v),
        Node::Edge(e, _) => (0..k.len()).any(|i| edge_key(k[i], k[(i + 1) % k.len()]) == e),
        Node::Inside(g, _) => g == f,
    }
}

/// The faces a node touches.
fn faces_at(c: &Cage, by_corner: &HashMap<u32, Vec<usize>>, n: Node) -> Vec<usize> {
    match n {
        Node::Point(v) => by_corner.get(&v).cloned().unwrap_or_default(),
        Node::Edge([a, _], _) => {
            by_corner.get(&a).into_iter().flatten().copied().filter(|&f| on_face(c, f, n)).collect()
        }
        Node::Inside(f, _) => vec![f],
    }
}

/// Whether `p` (near face `f`'s plane) is inside its polygon.
fn inside_face(c: &Cage, f: usize, p: Vec3) -> bool {
    let k = corners(&c.faces[f]);
    let n = face_normal(c, f);
    if n == Vec3::ZERO {
        return false;
    }
    let u = (c.positions[k[1] as usize] - c.positions[k[0] as usize]).normalize_or_zero();
    let w = n.cross(u);
    let flat = |q: Vec3| {
        let d = q - c.positions[k[0] as usize];
        (d.dot(u), d.dot(w))
    };
    let (px, py) = flat(p);
    let mut inside = false;
    for i in 0..k.len() {
        let (ax, ay) = flat(c.positions[k[i] as usize]);
        let (bx, by) = flat(c.positions[k[(i + 1) % k.len()] as usize]);
        if (ay > py) != (by > py) && px < ax + (py - ay) * (bx - ax) / (by - ay) {
            inside = !inside;
        }
    }
    inside
}

/// The line drawn on screen between two clicked points `a` and `b`, as the
/// plane through the eye and both (along the view, orthographic) that the
/// cut follows, and how far along that line a point on the plane is.
///
/// Why progress is measured on screen and not along the straight line from
/// `a` to `b` in space: a cut over a convex edge (across a cap and down a
/// side) crosses edges that lie further along that 3D line than `b` does,
/// although they come before it on screen, and the cut stopped there.
struct ScreenLine {
    a: Vec3,
    normal: Vec3,
    /// Perspective: the eye, and the plane's in-plane axes from it (the
    /// direction to `a` and the one at right angles toward `b`), scaled by
    /// the distance to `a` so progress is in about scene units near `a`.
    eye: Option<(Vec3, Vec3, Vec3, f32)>,
    /// Orthographic: the screen direction from `a` toward `b`.
    dir: Vec3,
}

impl ScreenLine {
    fn new(view: KnifeView, a: Vec3, b: Vec3) -> Option<Self> {
        let toward = if view.orthographic { view.forward } else { a - view.eye };
        let normal = (b - a).cross(toward).normalize_or_zero();
        if normal == Vec3::ZERO {
            return None;
        }
        if view.orthographic {
            let f = view.forward.normalize_or_zero();
            let d = (b - a) - f * (b - a).dot(f);
            return Some(ScreenLine { a, normal, eye: None, dir: d.normalize_or_zero() });
        }
        let ea = a - view.eye;
        let e1 = ea.normalize_or_zero();
        let e2 = ((b - view.eye) - e1 * (b - view.eye).dot(e1)).normalize_or_zero();
        if e1 == Vec3::ZERO || e2 == Vec3::ZERO {
            return None;
        }
        Some(ScreenLine { a, normal, eye: Some((view.eye, e1, e2, ea.length())), dir: Vec3::ZERO })
    }

    /// Which side of the cut's plane `p` is on (signed distance).
    fn side(&self, p: Vec3) -> f32 {
        (p - self.a).dot(self.normal)
    }

    /// How far along the screen line toward `b` the point `p` is seen.
    fn ahead(&self, p: Vec3) -> f32 {
        match self.eye {
            Some((eye, e1, e2, scale)) => {
                let r = p - eye;
                r.dot(e2).atan2(r.dot(e1)) * scale
            }
            None => (p - self.a).dot(self.dir),
        }
    }
}

/// Work out the cut along `points` in `view`. RETURNS: the plan, or why
/// there's nothing to cut.
pub fn knife_plan(c: &Cage, points: &[KnifePoint], view: KnifeView) -> Result<KnifePlan, &'static str> {
    if points.len() < 2 {
        return Err("click at least two points");
    }
    let mut by_corner: HashMap<u32, Vec<usize>> = HashMap::new();
    for (f, face) in c.faces.iter().enumerate() {
        for &v in corners(face) {
            by_corner.entry(v).or_default().push(f);
        }
    }
    let scale = {
        let (lo, hi) = c.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        (hi - lo).max_element().max(1e-6)
    };
    let eps = scale * 1e-5;
    let mut nodes: Vec<(Node, Option<usize>)> = vec![(snap_node(&points[0]), None)];
    for pair in points.windows(2) {
        let (a, b) = (pair[0].pos, pair[1].pos);
        let (start, end) = (snap_node(&pair[0]), snap_node(&pair[1]));
        let Some(line) = ScreenLine::new(view, a, b) else { continue };
        let side = |p: Vec3| line.side(p);
        let ahead = |p: Vec3| line.ahead(p);
        let total = ahead(b);
        let mut here = start;
        let mut from: Option<usize> = None;
        let mut done = false;
        for _ in 0..100_000 {
            let here_pos = node_pos(c, here);
            let s_here = ahead(here_pos);
            let mut candidates: Vec<usize> =
                faces_at(c, &by_corner, here).into_iter().filter(|&f| Some(f) != from).collect();
            // The knife cuts what you see: faces turned toward the eye
            // first, so at a border between a front and a back face the
            // path carries on over the front one.
            let facing = |f: usize| {
                let n = face_normal(c, f);
                let to_eye = if view.orthographic { -view.forward } else { view.eye - here_pos };
                n.dot(to_eye) > 0.0
            };
            if candidates.iter().any(|&f| facing(f)) {
                candidates.retain(|&f| facing(f));
            }
            // The end is on one of these faces, along the path: done.
            if let Some(&f) = candidates
                .iter()
                .find(|&&f| on_face(c, f, end) && inside_face(c, f, here_pos.lerp(b, 0.5)))
                .or_else(|| candidates.iter().find(|&&f| on_face(c, f, end)))
            {
                nodes.push((end, Some(f)));
                done = true;
                break;
            }
            // Otherwise the nearest crossing ahead, in the face the path
            // runs through.
            let mut best: Option<(f32, Node, usize)> = None;
            for &f in &candidates {
                let k = corners(&c.faces[f]);
                for i in 0..k.len() {
                    let (p, q) = (k[i], k[(i + 1) % k.len()]);
                    let (pp, qp) = (c.positions[p as usize], c.positions[q as usize]);
                    let (dp, dq) = (side(pp), side(qp));
                    let node = if dp.abs() <= eps {
                        Node::Point(p)
                    } else if (dp < 0.0) != (dq < 0.0) && dq.abs() > eps {
                        let t = dp / (dp - dq);
                        let [lo, hi] = edge_key(p, q);
                        let t = if lo == p { t } else { 1.0 - t };
                        Node::Edge([lo, hi], t)
                    } else {
                        continue;
                    };
                    if node == here || matches!((node, here), (Node::Point(v), Node::Edge(e, _)) if e.contains(&v)) {
                        continue;
                    }
                    if let (Node::Edge(e, _), Node::Edge(h, _)) = (node, here)
                        && e == h
                    {
                        continue;
                    }
                    let pos = node_pos(c, node);
                    let s = ahead(pos);
                    if s <= s_here + eps || s > total + eps {
                        continue;
                    }
                    if !inside_face(c, f, here_pos.lerp(pos, 0.5)) {
                        continue;
                    }
                    if best.is_none_or(|(bs, ..)| s < bs) {
                        best = Some((s, node, f));
                    }
                }
            }
            let Some((_, node, f)) = best else { break };
            nodes.push((node, Some(f)));
            here = node;
            from = Some(f);
        }
        if !done {
            // The path left the surface (an open border) or couldn't be
            // followed: the cut stops where it got to.
            break;
        }
    }
    // An end clicked inside a face (not on an edge or corner) carries the
    // cut straight on to that face's border, so the face is split all the
    // way across instead of the end being dropped (a cut from one face into
    // the next, or across one face, used to cut nothing). Not for a closed
    // loop, whose ends meet.
    let closed = points.len() >= 3 && snap_node(&points[0]) == snap_node(&points[points.len() - 1]);
    let mut extended = (false, false);
    if !closed && nodes.len() >= 2 {
        if let (Node::Inside(f, p), Some(next)) = (nodes[0].0, nodes.get(1).map(|n| node_pos(c, n.0)))
            && let Some(node) = border_along(c, f, view, next, p, eps)
        {
            nodes[0].1 = Some(f);
            nodes.insert(0, (node, None));
            extended.0 = true;
        }
        let last = nodes.len() - 1;
        if let (Node::Inside(f, p), Some(before)) = (nodes[last].0, nodes.get(last - 1).map(|n| node_pos(c, n.0)))
            && let Some(node) = border_along(c, f, view, before, p, eps)
        {
            nodes.push((node, Some(f)));
            extended.1 = true;
        }
    }
    if nodes.len() < 2 {
        return Err("the path doesn't cross the model");
    }
    Ok(KnifePlan { nodes, extended })
}

/// Where the line from `from` through `to` (cut along the plane through
/// the eye and both) leaves face `f` past `to`: the nearest crossing of one
/// of its edges, or a corner on the line, beyond `to`.
fn border_along(c: &Cage, f: usize, view: KnifeView, from: Vec3, to: Vec3, eps: f32) -> Option<Node> {
    let line = ScreenLine::new(view, from, to)?;
    let side = |p: Vec3| line.side(p);
    let ahead = |p: Vec3| line.ahead(p);
    let s_to = ahead(to);
    let k = corners(&c.faces[f]);
    let mut best: Option<(f32, Node)> = None;
    for i in 0..k.len() {
        let (p, q) = (k[i], k[(i + 1) % k.len()]);
        let (pp, qp) = (c.positions[p as usize], c.positions[q as usize]);
        let (dp, dq) = (side(pp), side(qp));
        let node = if dp.abs() <= eps {
            Node::Point(p)
        } else if (dp < 0.0) != (dq < 0.0) && dq.abs() > eps {
            let t = dp / (dp - dq);
            let [lo, hi] = edge_key(p, q);
            Node::Edge([lo, hi], if lo == p { t } else { 1.0 - t })
        } else {
            continue;
        };
        let s = ahead(node_pos(c, node));
        if s > s_to + eps && best.is_none_or(|(b, _)| s < b) {
            best = Some((s, node));
        }
    }
    best.map(|(_, n)| n)
}

/// The face and the points of a plan that is a closed loop of points all
/// clicked inside one face (the first point clicked again to close it).
fn inner_loop(plan: &KnifePlan) -> Option<(usize, Vec<Vec3>)> {
    let n = &plan.nodes;
    if n.len() < 4 || n[0].0 != n[n.len() - 1].0 {
        return None;
    }
    let Node::Inside(f, _) = n[0].0 else { return None };
    let points: Option<Vec<Vec3>> = n[..n.len() - 1]
        .iter()
        .map(|(node, _)| match *node {
            Node::Inside(g, p) if g == f => Some(p),
            _ => None,
        })
        .collect();
    points.map(|p| (f, p))
}

/// Intersections of planar boundary segments, including touches. Inputs
/// are normalized to the size of the parent face.
fn segments_meet(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    const TOL: f64 = 1e-10;
    if a.min(b).cmpgt(c.max(d) + DVec2::splat(TOL)).any() || c.min(d).cmpgt(a.max(b) + DVec2::splat(TOL)).any() {
        return false;
    }
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let across = |x, y| (x <= TOL && y >= -TOL) || (y <= TOL && x >= -TOL);
    across(side(a, b, c), side(a, b, d)) && across(side(c, d, a), side(c, d, b))
}

fn inside_polygon(p: DVec2, ring: &[DVec2]) -> bool {
    let mut inside = false;
    for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// A ring around a hole can be divided into simple polygons by joining
/// corresponding corners of its two outlines. Give every corner of the
/// smaller outline a distinct partner when possible. Extra corners stay
/// along an existing side of a rim polygon instead of fanning into triangles.
fn inset_spokes(outer: &[DVec2], inner: &[DVec2]) -> Option<Vec<(usize, usize)>> {
    let m = outer.len();
    let k = inner.len();
    let clear = |(o, i): (usize, usize)| {
        let (a, b) = (outer[o], inner[i]);
        let crosses = |ring: &[DVec2], at: usize| {
            (0..ring.len()).any(|j| {
                let next = (j + 1) % ring.len();
                j != at && next != at && segments_meet(a, b, ring[j], ring[next])
            })
        };
        let mid = (a + b) * 0.5;
        inside_polygon(mid, outer) && !inside_polygon(mid, inner) && !crosses(outer, o) && !crosses(inner, i)
    };
    let no_cross = |spokes: &[(usize, usize)]| {
        spokes.iter().enumerate().all(|(i, &(o, k))| {
            spokes[..i].iter().all(|&(p, l)| !segments_meet(outer[o], inner[k], outer[p], inner[l]))
        })
    };

    // Try each cyclic alignment, favoring short joins. Increasing offsets
    // preserve the outline order and pair the square-in-quad case one to one.
    let n = m.min(k);
    let larger = m.max(k);
    let pairs = |offset: usize| {
        (0..n).map(move |i| {
            let j = (offset + i * larger / n) % larger;
            if m >= k { (j, i) } else { (i, j) }
        })
    };
    let mut offsets: Vec<_> = (0..larger)
        .map(|offset| {
            let distance: f64 = pairs(offset).map(|(o, i)| outer[o].distance_squared(inner[i])).sum();
            (offset, distance)
        })
        .collect();
    offsets.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (offset, _) in offsets {
        let spokes: Vec<_> = pairs(offset).collect();
        if spokes.iter().copied().all(clear) && no_cross(&spokes) {
            return Some(spokes);
        }
    }

    // Unevenly spaced or concave boundaries may need fewer joins. Use
    // the nearest clear corner at each point, retaining only one spoke
    // per outer corner, and only accept a single forward boundary walk.
    let mut nearest = vec![None; m];
    for (i, &p) in inner.iter().enumerate() {
        let best = outer
            .iter()
            .enumerate()
            .map(|(o, q)| (o, p.distance_squared(*q)))
            .filter(|&(o, _)| clear((o, i)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((o, d)) = best
            && nearest[o].is_none_or(|(_, best)| d < best)
        {
            nearest[o] = Some((i, d));
        }
    }
    let spokes: Vec<_> = nearest.into_iter().enumerate().filter_map(|(o, hit)| hit.map(|(i, _)| (o, i))).collect();
    if spokes.len() < 2 {
        return None;
    }
    let walked: usize =
        spokes.iter().zip(spokes.iter().cycle().skip(1)).map(|(&(_, i), &(_, j))| (j + k - i) % k).sum();
    (walked == k && no_cross(&spokes)).then_some(spokes)
}

/// Cut a closed loop inside one face, keeping the loop as a polygon and
/// filling the rim with quads or larger polygons.
fn cut_inner_loop(c: &mut Cage, f: usize, points: &[Vec3]) -> Result<usize, &'static str> {
    if points.len() < 3 {
        return Err("click at least three points inside the face for a loop");
    }
    let normal = face_normal(c, f);
    if normal == Vec3::ZERO {
        return Err("the face is too thin to cut a loop in");
    }
    let outer = corners(&c.faces[f]).to_vec();
    let mid = c.positions[outer[0] as usize];
    let scale = outer.iter().map(|&v| c.positions[v as usize].distance(mid)).fold(0.0, f32::max);
    let (u, w) = normal.any_orthonormal_pair();
    let project = |p: Vec3| {
        let d = (p - mid) / scale;
        DVec2::new(d.dot(u) as f64, d.dot(w) as f64)
    };
    let outer_2d: Vec<_> = outer.iter().map(|&i| project(c.positions[i as usize])).collect();
    let mut inner = points.to_vec();
    if crate::retopo::polygon_normal(&inner).dot(normal) < 0.0 {
        inner.reverse();
    }
    let inner_2d: Vec<_> = inner.iter().copied().map(project).collect();
    let area: f64 = inner_2d.iter().zip(inner_2d.iter().cycle().skip(1)).map(|(&a, &b)| a.perp_dot(b)).sum();
    if area <= 1e-12 {
        return Err("draw a simple loop with area");
    }
    for (i, (&a, &b)) in inner_2d.iter().zip(inner_2d.iter().cycle().skip(1)).enumerate() {
        if a.distance_squared(b) <= 1e-14
            || !inside_polygon(a, &outer_2d)
            || outer_2d.iter().zip(outer_2d.iter().cycle().skip(1)).any(|(&p, &q)| segments_meet(a, b, p, q))
            || (0..i.saturating_sub(1))
                .filter(|&j| i + 1 < inner.len() || j != 0)
                .any(|j| segments_meet(a, b, inner_2d[j], inner_2d[j + 1]))
        {
            return Err("draw a simple loop entirely inside the face");
        }
    }
    let spokes =
        inset_spokes(&outer_2d, &inner_2d).ok_or("couldn't join the loop to the face without crossing edges")?;
    let group = c.faces[f].group;
    let inner: Vec<_> = inner.into_iter().map(|p| c.add_vertex(p)).collect();
    let arc = |ring: &[u32], from: usize, to: usize| {
        let n = ring.len();
        ring.iter().copied().cycle().skip(from).take((to + n - from) % n + 1).collect::<Vec<_>>()
    };
    let make_face = |mut corners: Vec<u32>| {
        // The mesh splits quads along corners 0–2. For a concave quad,
        // choose a starting corner that puts that split inside the face.
        if let &[a, b, d, e] = corners.as_slice() {
            let [first, second, third, fourth] = [a, b, d, e].map(|v| project(c.positions[v as usize]));
            let diagonal = third - first;
            if (second - first).perp_dot(diagonal) <= 0.0 || diagonal.perp_dot(fourth - first) <= 0.0 {
                corners.rotate_left(1);
            }
        }
        Face::with_group(corners, group)
    };
    for (&(a, i), &(b, j)) in spokes.iter().zip(spokes.iter().cycle().skip(1)) {
        let mut face = arc(&outer, a, b);
        face.extend(arc(&inner, i, j).into_iter().rev());
        c.faces.push(make_face(face));
    }
    c.faces[f] = make_face(inner);
    Ok(spokes.len())
}

/// Cut the cage along `plan`. RETURNS: how many faces the cut added.
pub fn knife_cut(c: &mut Cage, plan: &KnifePlan) -> Result<usize, &'static str> {
    // A closed loop clicked inside one face, touching none of its edges,
    // cuts a new face out of it (#450).
    if let Some((f, points)) = inner_loop(plan) {
        return cut_inner_loop(c, f, &points);
    }
    // Chains: a point on a face's border, the points inside that face, and
    // the next point on its border, with the face they cross.
    let mut chains: Vec<Vec<Node>> = Vec::new();
    let mut open: Option<(Vec<Node>, Option<usize>)> = None;
    for &(node, face) in &plan.nodes {
        let boundary = !matches!(node, Node::Inside(..));
        if let Some((mut chain, f)) = open.take() {
            // Every piece of a chain runs through the same face.
            if face.is_some() && (f.is_none() || f == face) {
                chain.push(node);
                if boundary {
                    chains.push(chain);
                } else {
                    open = Some((chain, face));
                }
            }
        }
        // A point on a border starts the next chain (an end inside a face,
        // with no border before it, is dropped).
        if boundary && open.is_none() {
            open = Some((vec![node], None));
        }
    }
    // Points on edges become corners of the faces around them; several on
    // one edge are put in order along it.
    let mut on_edge: HashMap<[u32; 2], Vec<(f32, u32)>> = HashMap::new();
    let mut vertex = |c: &mut Cage, n: Node| -> u32 {
        match n {
            Node::Point(v) => v,
            Node::Inside(_, p) => c.add_vertex(p),
            Node::Edge(e, t) => {
                let list = on_edge.entry(e).or_default();
                if let Some(&(_, v)) = list.iter().find(|(s, _)| (s - t).abs() < 1e-4) {
                    return v;
                }
                let v = c.add_vertex(c.positions[e[0] as usize].lerp(c.positions[e[1] as usize], t));
                let lo = list.iter().filter(|(s, _)| *s < t).max_by(|x, y| x.0.total_cmp(&y.0)).map_or(e[0], |x| x.1);
                let hi = list.iter().filter(|(s, _)| *s > t).min_by(|x, y| x.0.total_cmp(&y.0)).map_or(e[1], |x| x.1);
                list.push((t, v));
                for face in &mut c.faces {
                    let k = &mut face.corners;
                    let n = k.len();
                    if let Some(i) = (0..n).find(|&i| edge_key(k[i], k[(i + 1) % n]) == edge_key(lo, hi)) {
                        k.insert(i + 1, v);
                    }
                }
                let w = c.crease(e[0], e[1]);
                if w > 0.0 {
                    c.set_crease(lo, v, w);
                    c.set_crease(v, hi, w);
                }
                v
            }
        }
    };
    let chains: Vec<Vec<u32>> =
        chains.into_iter().map(|nodes| nodes.into_iter().map(|n| vertex(c, n)).collect()).collect();
    let mut made = 0;
    for chain in chains {
        let (b0, b1) = (chain[0], *chain.last().expect("chain"));
        let inner = &chain[1..chain.len() - 1];
        if b0 == b1 {
            continue;
        }
        let mid = chain.iter().map(|&v| c.positions[v as usize]).sum::<Vec3>() / chain.len() as f32;
        let holds = |f: &Face| corners(f).contains(&b0) && corners(f).contains(&b1);
        let mut found: Vec<usize> = (0..c.faces.len()).filter(|&f| holds(&c.faces[f])).collect();
        found.sort_by_key(|&f| !inside_face(c, f, mid));
        let Some(&f) = found.first() else { continue };
        let k = corners(&c.faces[f]).to_vec();
        let n = k.len();
        let i = k.iter().position(|&v| v == b0).expect("corner");
        let j = k.iter().position(|&v| v == b1).expect("corner");
        if inner.is_empty() && ((i + 1) % n == j || (j + 1) % n == i) {
            continue;
        }
        let walk = |from: usize, to: usize| -> Vec<u32> {
            let mut out = vec![k[from]];
            let mut x = from;
            while x != to {
                x = (x + 1) % n;
                out.push(k[x]);
            }
            out
        };
        let mut one = walk(i, j);
        one.extend(inner.iter().rev());
        let mut two = walk(j, i);
        two.extend(inner.iter());
        if one.len() < 3 || two.len() < 3 {
            continue;
        }
        let group = c.faces[f].group;
        c.faces[f] = Face::with_group(one, group);
        c.faces.push(Face::with_group(two, group));
        made += 1;
    }
    if made == 0 {
        return Err("nothing to cut: the path must cross a face from one side to another");
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyedit::cage_from_mesh;

    /// A flat 3×3 grid of quads in the XY plane, 0..3 on each side, seen
    /// from +Z.
    fn grid() -> Cage {
        let mut c = Cage::default();
        for y in 0..4 {
            for x in 0..4 {
                c.add_vertex(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        for y in 0..3u32 {
            for x in 0..3u32 {
                let v = y * 4 + x;
                c.faces.push(Face::new(vec![v, v + 1, v + 5, v + 4]));
            }
        }
        c
    }

    fn view() -> KnifeView {
        KnifeView { eye: Vec3::new(1.5, 1.5, 10.0), forward: Vec3::NEG_Z, orthographic: false }
    }

    fn face_at(c: &Cage, p: Vec3) -> usize {
        (0..c.faces.len()).find(|&f| inside_face(c, f, p)).expect("a face")
    }

    fn inside(c: &Cage, p: Vec3) -> KnifePoint {
        KnifePoint { snap: KnifeSnap::Face(face_at(c, p)), pos: p }
    }

    #[test]
    fn a_cut_across_the_grid_splits_each_face_it_crosses() {
        // From the left border to the right border along y = 1.5.
        let mut c = grid();
        let a = KnifePoint { snap: KnifeSnap::Edge([4, 8], 0.5), pos: Vec3::new(0.0, 1.5, 0.0) };
        let b = KnifePoint { snap: KnifeSnap::Edge([7, 11], 0.5), pos: Vec3::new(3.0, 1.5, 0.0) };
        let plan = knife_plan(&c, &[a, b], view()).unwrap();
        assert_eq!(plan.crossings(), 4, "both ends and the two edges between");
        let made = knife_cut(&mut c, &plan).unwrap();
        assert_eq!(made, 3);
        assert_eq!(c.faces.len(), 12);
        assert_eq!(c.positions.len(), 16 + 4);
        crate::polyedit::validate(&c).unwrap();
        // Every face is a quad, and the new points are on y = 1.5.
        assert!(c.faces.iter().all(|f| f.len() == 4));
        assert!(c.positions[16..].iter().all(|p| (p.y - 1.5).abs() < 1e-5));
    }

    #[test]
    fn points_inside_a_face_become_corners_and_an_end_inside_a_face_cuts_on_to_its_border() {
        let mut c = grid();
        // From the middle face's left edge, bend inside it, leave through
        // its right edge and end inside the next face: the middle face is
        // split with the bend as a corner, and the end carries on to the
        // right border, splitting that face too.
        let pts = [
            KnifePoint { snap: KnifeSnap::Edge([5, 9], 0.5), pos: Vec3::new(1.0, 1.5, 0.0) },
            inside(&c, Vec3::new(1.5, 1.7, 0.0)),
            inside(&c, Vec3::new(2.5, 1.5, 0.0)),
        ];
        let plan = knife_plan(&c, &pts, view()).unwrap();
        let made = knife_cut(&mut c, &plan).unwrap();
        assert_eq!(made, 2);
        crate::polyedit::validate(&c).unwrap();
        let corner_of = |c: &Cage, p: Vec3| {
            let v = c.positions.iter().position(|q| q.distance(p) < 1e-4).expect("a point there") as u32;
            c.faces.iter().filter(|f| f.corners.contains(&v)).count()
        };
        assert_eq!(corner_of(&c, Vec3::new(1.5, 1.7, 0.0)), 2, "the bend");
        assert_eq!(corner_of(&c, Vec3::new(2.5, 1.5, 0.0)), 2, "the end inside the face");
        assert!(c.positions.iter().any(|p| (p.x - 3.0).abs() < 1e-4 && p.y > 1.0 && p.y < 2.0), "on the border");
    }

    #[test]
    fn clicks_inside_faces_still_cut() {
        // Both inside one face: it is split across, from border to border.
        let mut c = grid();
        let pts = [inside(&c, Vec3::new(1.3, 1.5, 0.0)), inside(&c, Vec3::new(1.7, 1.5, 0.0))];
        let plan = knife_plan(&c, &pts, view()).unwrap();
        let made = knife_cut(&mut c, &plan).unwrap();
        assert_eq!(made, 1);
        assert_eq!(c.faces.len(), 10);
        crate::polyedit::validate(&c).unwrap();
        // Inside two neighbouring faces: both are split.
        let mut c = grid();
        let pts = [inside(&c, Vec3::new(1.5, 1.5, 0.0)), inside(&c, Vec3::new(2.5, 1.5, 0.0))];
        let plan = knife_plan(&c, &pts, view()).unwrap();
        let made = knife_cut(&mut c, &plan).unwrap();
        assert_eq!(made, 2);
        crate::polyedit::validate(&c).unwrap();
        // The clicked points stay as corners of the halves (four
        // pentagons), and the left face gains a corner where the cut lands.
        assert_eq!(c.faces.iter().filter(|f| f.len() == 5).count(), 5);
    }

    #[test]
    fn a_cut_follows_the_surface_over_a_cube_edge() {
        // From the middle of the front face's bottom edge, over the top
        // edge, to the middle of the top face's back edge.
        let mut cube = cage_from_mesh(&crate::primitives::cube(1, 1.0));
        let at = |c: &Cage, p: Vec3| c.positions.iter().position(|q| q.distance(p) < 1e-4).unwrap() as u32;
        let edge_mid = |c: &Cage, a: Vec3, b: Vec3| {
            let (va, vb) = (at(c, a), at(c, b));
            KnifePoint { snap: KnifeSnap::Edge([va, vb], 0.5), pos: (a + b) * 0.5 }
        };
        let start = edge_mid(&cube, Vec3::new(-0.5, -0.5, 0.5), Vec3::new(0.5, -0.5, 0.5));
        let end = edge_mid(&cube, Vec3::new(-0.5, 0.5, -0.5), Vec3::new(0.5, 0.5, -0.5));
        let view = KnifeView {
            eye: Vec3::new(0.0, 2.0, 4.0),
            forward: Vec3::new(0.0, -0.4, -1.0).normalize(),
            orthographic: false,
        };
        let plan = knife_plan(&cube, &[start, end], view).unwrap();
        assert_eq!(plan.crossings(), 3, "start, the front-top edge, end");
        let faces = cube.faces.len();
        assert_eq!(knife_cut(&mut cube, &plan).unwrap(), 2);
        assert_eq!(cube.faces.len(), faces + 2);
        crate::polyedit::validate(&cube).unwrap();
        // The new points are on x = 0, one of them on the front-top edge.
        assert!(cube.positions[8..].iter().all(|p| p.x.abs() < 1e-4));
        assert!(cube.positions[8..].iter().any(|p| p.distance(Vec3::new(0.0, 0.5, 0.5)) < 1e-4));
    }

    #[test]
    fn a_cut_through_a_corner_and_a_diagonal_path_on_a_cube() {
        // From corner 5 diagonally to corner 10: splits the middle face
        // along its diagonal, no new points.
        let mut c = grid();
        let a = KnifePoint { snap: KnifeSnap::Point(5), pos: Vec3::new(1.0, 1.0, 0.0) };
        let b = KnifePoint { snap: KnifeSnap::Point(10), pos: Vec3::new(2.0, 2.0, 0.0) };
        let plan = knife_plan(&c, &[a, b], view()).unwrap();
        assert_eq!(knife_cut(&mut c, &plan).unwrap(), 1);
        assert_eq!(c.positions.len(), 16);
        assert_eq!(c.faces.iter().filter(|f| f.len() == 3).count(), 2);

        // Round a cube's front, top and back: the path follows the surface
        // across three faces.
        let mut cube = cage_from_mesh(&crate::primitives::cube(1, 1.0));
        let front = (0..cube.faces.len()).max_by(|&x, &y| face_normal(&cube, x).z.total_cmp(&face_normal(&cube, y).z));
        let front = front.unwrap();
        let top = (0..cube.faces.len()).max_by(|&x, &y| face_normal(&cube, x).y.total_cmp(&face_normal(&cube, y).y));
        let a = KnifePoint { snap: KnifeSnap::Face(front), pos: Vec3::new(0.0, -0.2, 0.5) };
        let b = KnifePoint { snap: KnifeSnap::Face(top.unwrap()), pos: Vec3::new(0.0, 0.5, 0.2) };
        let view = KnifeView { eye: Vec3::new(3.0, 3.0, 3.0), forward: Vec3::NEG_ONE.normalize(), orthographic: false };
        let plan = knife_plan(&cube, &[a, b], view).unwrap();
        assert!(plan.crossings() >= 1, "crosses the edge between front and top");
        // Both ends inside faces: each carries on to its face's border, so
        // the front and the top are both split.
        assert_eq!(knife_cut(&mut cube, &plan).unwrap(), 2);
        crate::polyedit::validate(&cube).unwrap();
    }
    /// An 8-sided prism (8-gon caps), y from -0.5 to 0.5, radius 1.
    fn prism() -> Cage {
        let n = 8u32;
        let mut c = Cage::default();
        for y in [-0.5f32, 0.5] {
            for k in 0..n {
                let a = std::f32::consts::TAU * k as f32 / n as f32;
                c.add_vertex(Vec3::new(a.cos(), y, -a.sin()));
            }
        }
        for k in 0..n {
            let j = (k + 1) % n;
            c.faces.push(Face::new(vec![k, j, n + j, n + k]));
        }
        c.faces.push(Face::new((0..n).rev().collect()));
        c.faces.push(Face::new((n..2 * n).collect()));
        c
    }

    #[test]
    fn a_cut_over_a_convex_edge_reaches_a_point_below_it() {
        // From a corner at the back of the top cap, across the cap and down
        // the front-right side. The rim it crosses lies further along the
        // straight line between the two points than the end does, although
        // it comes first on screen: the cut used to stop at the corner.
        let mut c = prism();
        let eye = Vec3::new(1.33832, 3.0982447, 4.3264246);
        let view = KnifeView { eye, forward: (-eye).normalize(), orthographic: false };
        let a = KnifePoint { snap: KnifeSnap::Point(11), pos: c.positions[11] };
        let side = 7; // corners 7, 0, 8, 15
        let pos = Vec3::new(0.8656472, 0.26541042, 0.32435584);
        let b = KnifePoint { snap: KnifeSnap::Face(side), pos };
        let plan = knife_plan(&c, &[a, b], view).unwrap();
        assert!(plan.path(&c).iter().any(|q| q.distance(pos) < 1e-4), "reaches the end: {:?}", plan.path(&c));
        assert_eq!(plan.extended(), (false, true), "only the end inside a face carries on");
        let made = knife_cut(&mut c, &plan).unwrap();
        assert!(made >= 2, "the top cap and the side: {made}");
        crate::polyedit::validate(&c).unwrap();
        assert!(c.faces.iter().filter(|f| f.len() >= 5).count() >= 2, "the cap halves are still polygons");
    }
    #[test]
    fn a_closed_loop_inside_a_face_cuts_a_new_face_out_of_it() {
        // #450: four clicks inside the middle quad of the grid and the first
        // again: a new quad in the middle, four quads around it.
        let mut c = grid();
        let f = face_at(&c, Vec3::new(1.5, 1.5, 0.0));
        let corners_at = [(1.2, 1.2), (1.8, 1.2), (1.8, 1.8), (1.2, 1.8)];
        let mut pts: Vec<KnifePoint> = corners_at
            .iter()
            .map(|&(x, y)| KnifePoint { snap: KnifeSnap::Face(f), pos: Vec3::new(x, y, 0.0) })
            .collect();
        pts.push(pts[0]);
        let plan = knife_plan(&c, &pts, view()).unwrap();
        assert_eq!(plan.path(&c).len(), 5, "the loop is the preview");
        let faces = c.faces.len();
        assert_eq!(knife_cut(&mut c, &plan).unwrap(), 4);
        assert_eq!(c.faces.len(), faces + 4);
        crate::polyedit::validate(&c).unwrap();
        assert!(c.faces.iter().all(|f| f.len() == 4), "all quads, as an inset");
        // The middle face is the loop, facing the same way as before.
        let inner = (0..c.faces.len()).find(|&g| c.faces[g].corners.iter().all(|&v| v >= 16)).unwrap();
        assert!(face_normal(&c, inner).z > 0.9);
        // Drawn the other way round (clockwise), the same.
        let mut c2 = grid();
        let mut back: Vec<KnifePoint> = pts[..4].iter().rev().copied().collect();
        back.push(back[0]);
        let plan = knife_plan(&c2, &back, view()).unwrap();
        assert_eq!(knife_cut(&mut c2, &plan).unwrap(), 4);
        crate::polyedit::validate(&c2).unwrap();
        // A triangle loop: three faces round it, one of them reaching two corners.
        let mut c3 = grid();
        let mut tri: Vec<KnifePoint> = [(1.3, 1.3), (1.7, 1.3), (1.5, 1.7)]
            .iter()
            .map(|&(x, y)| KnifePoint { snap: KnifeSnap::Face(f), pos: Vec3::new(x, y, 0.0) })
            .collect();
        tri.push(tri[0]);
        let plan = knife_plan(&c3, &tri, view()).unwrap();
        assert_eq!(knife_cut(&mut c3, &plan).unwrap(), 3);
        crate::polyedit::validate(&c3).unwrap();
    }

    #[test]
    fn mismatched_knife_loops_keep_extra_corners_in_rim_polygons() {
        let regular = |n: usize, radius: f32, offset: f32| {
            (0..n)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / n as f32 + offset;
                    Vec3::new(a.cos() * radius, a.sin() * radius, 0.0)
                })
                .collect::<Vec<_>>()
        };
        let xy = |points: &[(f32, f32)]| points.iter().map(|&(x, y)| Vec3::new(x, y, 0.0)).collect::<Vec<_>>();
        let offset_hexagon = (
            xy(&[(0.0, 0.0), (502.0, 0.0), (502.0, 363.0), (0.0, 363.0)]),
            xy(&[(162.0, 284.0), (64.0, 237.0), (64.0, 139.0), (162.0, 87.0), (260.0, 139.0), (260.0, 237.0)]),
        );
        let centered = [(4, 6), (6, 4), (4, 4)]
            .map(|(m, k)| (regular(m, 2.0, std::f32::consts::FRAC_PI_4), regular(k, 0.65, 0.16)));
        for (outer, inner) in centered.into_iter().chain([offset_hexagon]) {
            let (m, k) = (outer.len(), inner.len());
            let outer_area = crate::retopo::polygon_normal(&outer).z;
            let mut c = Cage {
                positions: outer,
                faces: vec![Face::with_group((0..m as u32).collect(), 17)],
                ..Default::default()
            };
            let mut clicks: Vec<_> = inner.iter().map(|&pos| KnifePoint { snap: KnifeSnap::Face(0), pos }).collect();
            clicks.push(clicks[0]);
            let plan = knife_plan(&c, &clicks, view()).unwrap();
            assert_eq!(knife_cut(&mut c, &plan), Ok(m.min(k)));
            crate::polyedit::validate(&c).unwrap();
            assert_eq!(c.faces[0].len(), k, "the drawn loop stays intact");
            assert!(c.faces.iter().all(|f| f.len() >= 4 && f.group == 17));
            let (triangles, _) = c.triangulate();
            let mut area = 0.0;
            for triangle in triangles {
                let [a, b, d] = triangle.map(|v| c.positions[v as usize]);
                let twice_area = (b - a).cross(d - a).z;
                assert!(twice_area > 0.0, "the rendered triangles keep the face winding");
                area += twice_area;
            }
            assert!((area / outer_area - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn a_loop_that_folds_back_on_itself_is_refused() {
        let mut c = grid();
        let f = face_at(&c, Vec3::new(1.5, 1.5, 0.0));
        // A bow tie: it crosses itself.
        let mut pts: Vec<KnifePoint> = [(1.2, 1.2), (1.8, 1.8), (1.8, 1.2), (1.2, 1.8)]
            .iter()
            .map(|&(x, y)| KnifePoint { snap: KnifeSnap::Face(f), pos: Vec3::new(x, y, 0.0) })
            .collect();
        pts.push(pts[0]);
        let plan = knife_plan(&c, &pts, view()).unwrap();
        let before = c.faces.len();
        assert!(knife_cut(&mut c, &plan).is_err());
        assert_eq!(c.faces.len(), before, "unchanged");
    }
}
