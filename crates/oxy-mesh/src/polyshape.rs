//! Poly Edit shaping operations (RFC 0018, phase B): bevel, bridge, split,
//! spin, edge slide and equalize, on a [`Cage`].
//!
//! Like [`crate::polyedit`], an interactive operation changes the topology
//! once and returns a [`Slide`]: the new points start where the old ones
//! were, and the drag only moves them. Every operation keeps the module
//! INVARIANT of [`crate::polyedit`] (checked by `validate` in the tests):
//! faces with three or more distinct corners, at most two faces per edge,
//! consistent winding, face groups kept, new faces in a new group.
//!
//! These are the textbook polygon modeling operations; nothing here is
//! taken from another program's code.

use crate::polyedit::{Adjacency, Slide, face_normal, new_group, remove_unused_points};
use crate::retopo::{Cage, Face, corners, edge_key};
use glam::Vec3;
use std::collections::{HashMap, HashSet};

/// The face running each directed edge `(a, b)` (corner `a` then `b`).
fn directed_edges(c: &Cage) -> HashMap<(u32, u32), usize> {
    let mut out = HashMap::new();
    for (f, face) in c.faces.iter().enumerate() {
        let k = corners(face);
        for i in 0..k.len() {
            out.insert((k[i], k[(i + 1) % k.len()]), f);
        }
    }
    out
}

/// The corners before and after `v` in face `f`.
fn around(c: &Cage, f: usize, v: u32) -> Option<(u32, u32)> {
    let k = corners(&c.faces[f]);
    let i = k.iter().position(|&x| x == v)?;
    Some((k[(i + k.len() - 1) % k.len()], k[(i + 1) % k.len()]))
}

/// The faces around point `v` in order, each followed by the one across its
/// edge leaving `v` (the edge `v -> next corner`). For a point on a border
/// the list starts at the border, so it runs from one border edge to the
/// other; `closed` says whether it wraps around.
fn fan(c: &Cage, de: &HashMap<(u32, u32), usize>, faces_at: &[usize], v: u32) -> (Vec<usize>, bool) {
    let next = |f: usize| -> Option<usize> {
        let (_, n) = around(c, f, v)?;
        de.get(&(n, v)).copied()
    };
    let prev = |f: usize| -> Option<usize> {
        let (p, _) = around(c, f, v)?;
        de.get(&(v, p)).copied()
    };
    let Some(&first) = faces_at.first() else { return (Vec::new(), false) };
    // Walk back to a border, if there is one.
    let mut start = first;
    let mut steps = 0;
    while let Some(p) = prev(start) {
        if p == first || steps > faces_at.len() {
            break;
        }
        start = p;
        steps += 1;
    }
    let closed = prev(start).is_some();
    let mut out = vec![start];
    let mut at = start;
    while let Some(n) = next(at) {
        if n == start || out.len() > faces_at.len() {
            break;
        }
        out.push(n);
        at = n;
    }
    (out, closed)
}

/// Bevel `edges`: each edge becomes a strip face between its two faces,
/// which move back from it as the slide amount grows (the amount is the
/// distance, along the edges leaving each end). Where three or more
/// bevelled edges meet, a corner face fills the gap.
///
/// How: around each end point, the bevelled edges cut its faces into
/// sectors, and each sector gets one new point, used by every face in the
/// sector. A sector with one edge that isn't bevelled slides along it; a
/// sector that is a single face between two bevelled edges moves into the
/// face, along both; a sector with more stays where the point is (a bevel
/// that ends there narrows to a point). Border edges and edges with the
/// same face on both sides are skipped.
///
/// RETURNS: the points to slide and the new faces (strips and corners, in
/// one new face group).
pub fn bevel_edges(c: &mut Cage, edges: &[[u32; 2]]) -> (Slide, Vec<usize>) {
    let de = directed_edges(c);
    let adj = Adjacency::new(c);
    let selected: HashSet<[u32; 2]> = edges
        .iter()
        .map(|&[a, b]| edge_key(a, b))
        .filter(|&[a, b]| {
            let fs = adj.faces_of_edge(a, b);
            fs.len() == 2 && fs[0] != fs[1]
        })
        .collect();
    let mut slide = Slide::default();
    if selected.is_empty() {
        return (slide, Vec::new());
    }
    let mut faces_at: HashMap<u32, Vec<usize>> = HashMap::new();
    for (f, face) in c.faces.iter().enumerate() {
        for &v in corners(face) {
            faces_at.entry(v).or_default().push(f);
        }
    }
    let mut ends: Vec<u32> = selected.iter().flatten().copied().collect();
    ends.sort_unstable();
    ends.dedup();
    // (face, old corner) -> new corner, and the corner faces to add.
    let mut corner_of: HashMap<(usize, u32), u32> = HashMap::new();
    let mut patches: Vec<Vec<u32>> = Vec::new();
    // Ends of a single bevelled edge inside the surface: (face, corner,
    // new point, before) to insert next to the corner, the corners to drop,
    // and the ends whose old point stays in the strip.
    let mut inserts: Vec<(usize, u32, u32, bool)> = Vec::new();
    let mut drops: Vec<(usize, u32)> = Vec::new();
    let mut kept_end: HashSet<u32> = HashSet::new();
    for v in ends {
        let (order, closed) = fan(c, &de, &faces_at[&v], v);
        let is_sel = |a: u32, b: u32| selected.contains(&edge_key(a, b));
        let bevelled_here = order
            .iter()
            .filter(|&&f| {
                let (_, n) = around(c, f, v).expect("in fan");
                is_sel(v, n)
            })
            .count();
        if closed && bevelled_here == 1 && order.len() >= 3 {
            // One bevelled edge v-o ends here, so it cuts the fan only once.
            // P runs v -> o, Q runs o -> v. Each gets its own point, sliding
            // along its other edge at v; the faces across those edges take
            // the points in (between their corner v and the far point). With
            // three faces round v, the one left takes both and v goes.
            let o = order
                .iter()
                .find_map(|&f| {
                    let (_, n) = around(c, f, v)?;
                    is_sel(v, n).then_some(n)
                })
                .expect("one bevelled edge");
            let (Some(&pf), Some(&qf)) = (de.get(&(v, o)), de.get(&(o, v))) else { continue };
            let (xp, _) = around(c, pf, v).expect("corner");
            let (_, xq) = around(c, qf, v).expect("corner");
            let (Some(&hp), Some(&hq)) = (de.get(&(v, xp)), de.get(&(xq, v))) else { continue };
            let at = c.positions[v as usize];
            let mut point = |x: u32| {
                let new = c.add_vertex(at);
                let d = (c.positions[x as usize] - at).normalize_or_zero();
                slide.push(c, new, d);
                new
            };
            let (np, nq) = (point(xp), point(xq));
            corner_of.insert((pf, v), np);
            corner_of.insert((qf, v), nq);
            // hp runs v -> xp: v, np, xp. hq runs xq -> v: xq, nq, v.
            inserts.push((hp, v, np, false));
            inserts.push((hq, v, nq, true));
            if hp == hq {
                drops.push((hp, v));
            } else {
                kept_end.insert(v);
            }
            continue;
        }
        // Split the fan into sectors at bevelled edges (the edge leaving v
        // in a face's corner order is the one shared with the next face).
        let mut sectors: Vec<Vec<usize>> = vec![Vec::new()];
        // On a closed fan, start right after a bevelled edge.
        let rot = if closed {
            order
                .iter()
                .position(|&f| {
                    let (p, _) = around(c, f, v).expect("in fan");
                    is_sel(p, v)
                })
                .unwrap_or(0)
        } else {
            0
        };
        for i in 0..order.len() {
            let f = order[(i + rot) % order.len()];
            sectors.last_mut().expect("one at least").push(f);
            let (_, n) = around(c, f, v).expect("in fan");
            if is_sel(v, n) && i + 1 < order.len() {
                sectors.push(Vec::new());
            }
        }
        let mut points = Vec::with_capacity(sectors.len());
        for sector in &sectors {
            // The sector's edges at v that aren't bevelled: those between
            // its faces, and a border edge at either end.
            let mut free: Vec<u32> = Vec::new();
            for (i, &f) in sector.iter().enumerate() {
                let (p, n) = around(c, f, v).expect("in fan");
                if i == 0 && !is_sel(p, v) {
                    free.push(p);
                }
                if !is_sel(v, n) {
                    free.push(n);
                }
            }
            free.sort_unstable();
            free.dedup();
            let at = c.positions[v as usize];
            let dir = match free.len() {
                0 => {
                    let (p, n) = around(c, sector[0], v).expect("in fan");
                    let pp = (c.positions[p as usize] - at).normalize_or_zero();
                    let nn = (c.positions[n as usize] - at).normalize_or_zero();
                    Some(pp + nn)
                }
                1 => Some((c.positions[free[0] as usize] - at).normalize_or_zero()),
                _ => None,
            };
            let point = match dir {
                Some(d) => {
                    let new = c.add_vertex(at);
                    slide.push(c, new, d);
                    new
                }
                None => v,
            };
            for &f in sector {
                corner_of.insert((f, v), point);
            }
            points.push(point);
        }
        if closed && bevelled_here >= 3 {
            // Strips run each gap from one sector's point to the next in fan
            // order; the corner face runs them the other way.
            let mut patch: Vec<u32> = points.iter().rev().copied().collect();
            patch.dedup();
            if patch.len() >= 3 && patch.first() != patch.last() {
                patches.push(patch);
            }
        }
    }
    // Every face moves its corners to the new points.
    for (f, face) in c.faces.iter_mut().enumerate() {
        for v in face.corners.iter_mut() {
            if let Some(&n) = corner_of.get(&(f, *v)) {
                *v = n;
            }
        }
    }
    for (f, v, new, before) in inserts {
        let k = &mut c.faces[f].corners;
        if let Some(i) = k.iter().position(|&x| x == v) {
            k.insert(if before { i } else { i + 1 }, new);
        }
    }
    for (f, v) in drops {
        c.faces[f].corners.retain(|&x| x != v);
    }
    let group = new_group(c);
    let mut made = Vec::new();
    let mut sorted: Vec<[u32; 2]> = selected.into_iter().collect();
    sorted.sort_unstable();
    for [a, b] in sorted {
        let (Some(&f), Some(&g)) = (de.get(&(a, b)), de.get(&(b, a))) else { continue };
        let m = |face: usize, v: u32| corner_of.get(&(face, v)).copied().unwrap_or(v);
        // f runs a -> b, g runs b -> a; at an end whose old point stays,
        // the strip goes through it.
        let mut strip = vec![m(f, b), m(f, a)];
        if kept_end.contains(&a) {
            strip.push(a);
        }
        strip.extend([m(g, a), m(g, b)]);
        if kept_end.contains(&b) {
            strip.push(b);
        }
        strip.dedup();
        while strip.len() > 1 && strip.first() == strip.last() {
            strip.pop();
        }
        if strip.len() >= 3 {
            c.faces.push(Face::with_group(strip, group));
            made.push(c.faces.len() - 1);
        }
    }
    for patch in patches {
        c.faces.push(Face::with_group(patch, group));
        made.push(c.faces.len() - 1);
    }
    (slide, made)
}

/// Bevel `faces`: an extrude whose cap also shrinks inward as it goes out,
/// at 45° (the amount is how far out, and how far in from the edges). With
/// `each`, every face on its own. RETURNS: the points to slide and the new
/// wall faces.
pub fn bevel_faces(c: &mut Cage, faces: &[usize], each: bool) -> (Slide, Vec<usize>) {
    let regions: Vec<Vec<usize>> = if each { faces.iter().map(|&f| vec![f]).collect() } else { vec![faces.to_vec()] };
    // Inward direction of each outline point, per region, before the extrude.
    let mut inward: HashMap<u32, Vec3> = HashMap::new();
    for region in &regions {
        let set: HashSet<usize> = region.iter().copied().collect();
        let adj = Adjacency::new(c);
        let mut perps: HashMap<u32, Vec<Vec3>> = HashMap::new();
        for &f in region {
            let k = corners(&c.faces[f]);
            let n = face_normal(c, f);
            for i in 0..k.len() {
                let (a, b) = (k[i], k[(i + 1) % k.len()]);
                if adj.faces_of_edge(a, b).iter().any(|&g| g != f && set.contains(&g)) {
                    continue;
                }
                let e = (c.positions[b as usize] - c.positions[a as usize]).normalize_or_zero();
                let perp = n.cross(e).normalize_or_zero();
                perps.entry(a).or_default().push(perp);
                perps.entry(b).or_default().push(perp);
            }
        }
        for (v, ps) in perps {
            let m = ps.iter().copied().sum::<Vec3>().normalize_or_zero();
            let cos = ps.iter().map(|p| p.dot(m)).fold(1.0f32, f32::min).max(0.25);
            inward.insert(v, m / cos);
        }
    }
    let before = c.positions.len() as u32;
    let (mut slide, made) = crate::polyedit::extrude(c, faces, each);
    // The cap's outline points are copies made by the extrude, in the
    // order of the originals; find each copy's original by position.
    for (i, &v) in slide.verts.iter().enumerate() {
        if v < before {
            continue;
        }
        let at = slide.start[i];
        if let Some((_, d)) = inward.iter().find(|(o, _)| c.positions[**o as usize].distance_squared(at) <= 1e-12) {
            slide.dir[i] += *d;
        }
    }
    (slide, made)
}

/// Bridge faces `a` and `b`: both are removed and a tube of quads joins
/// their outlines, with `segments` rows (at least 1) and `twist` extra
/// steps of rotation. The corners are matched so the tube is as short as
/// possible before the twist.
///
/// RETURNS: the new faces (one new group), or why it can't be done.
pub fn bridge_faces(c: &mut Cage, a: usize, b: usize, segments: u32, twist: i32) -> Result<Vec<usize>, &'static str> {
    if a == b || a >= c.faces.len() || b >= c.faces.len() {
        return Err("pick two different faces");
    }
    let la = corners(&c.faces[a]).to_vec();
    let lb = corners(&c.faces[b]).to_vec();
    if la.len() != lb.len() {
        return Err("the two faces need the same number of corners");
    }
    if la.iter().any(|v| lb.contains(v)) {
        return Err("the faces touch; pick two faces that share no point");
    }
    let n = la.len();
    // Face b runs the other way round as seen across the bridge: corner i
    // of a meets corner (offset - i) of b.
    let at = |off: usize, i: usize| lb[(off + n * 2 - i % n) % n];
    let cost = |off: usize| -> f32 {
        (0..n).map(|i| c.positions[la[i] as usize].distance_squared(c.positions[at(off, i) as usize])).sum()
    };
    let best = (0..n).min_by(|&x, &y| cost(x).total_cmp(&cost(y))).expect("n >= 3");
    let off = (best as i32 + twist).rem_euclid(n as i32) as usize;
    let segments = segments.max(1);
    if segments == 1 {
        let existing: HashSet<[u32; 2]> = c.edges().into_iter().collect();
        if (0..n).any(|i| existing.contains(&edge_key(la[i], at(off, i)))) {
            return Err("their corners are already joined by edges; use 2 or more rows");
        }
    }
    let group = new_group(c);
    // Rings from a's outline (0) to b's (segments).
    let mut rings: Vec<Vec<u32>> = vec![la.clone()];
    for s in 1..segments {
        let t = s as f32 / segments as f32;
        let ring = (0..n)
            .map(|i| {
                let p = c.positions[la[i] as usize].lerp(c.positions[at(off, i) as usize], t);
                c.add_vertex(p)
            })
            .collect();
        rings.push(ring);
    }
    rings.push((0..n).map(|i| at(off, i)).collect());
    // Remove the two faces (the higher index first, so the other keeps its).
    for f in [a.max(b), a.min(b)] {
        c.faces.remove(f);
    }
    let mut made = Vec::new();
    for r in 0..segments as usize {
        let (lo, hi) = (&rings[r], &rings[r + 1]);
        for i in 0..n {
            let j = (i + 1) % n;
            c.faces.push(Face::with_group(vec![lo[i], lo[j], hi[j], hi[i]], group));
            made.push(c.faces.len() - 1);
        }
    }
    Ok(made)
}

/// Split the face that has both `a` and `b` as corners (not next to each
/// other) into two along a new edge `a`-`b`. Both halves keep the face's
/// group. RETURNS: the index of the second half, or None.
pub fn split_face(c: &mut Cage, a: u32, b: u32) -> Option<usize> {
    let f = c.faces.iter().position(|face| {
        let k = corners(face);
        let (Some(i), Some(j)) = (k.iter().position(|&v| v == a), k.iter().position(|&v| v == b)) else {
            return false;
        };
        let d = (j + k.len() - i) % k.len();
        d >= 2 && d <= k.len() - 2
    })?;
    let k = corners(&c.faces[f]).to_vec();
    let i = k.iter().position(|&v| v == a)?;
    let j = k.iter().position(|&v| v == b)?;
    let walk = |from: usize, to: usize| -> Vec<u32> {
        let mut out = Vec::new();
        let mut x = from;
        loop {
            out.push(k[x]);
            if x == to {
                break;
            }
            x = (x + 1) % k.len();
        }
        out
    };
    let group = c.faces[f].group;
    c.faces[f] = Face::with_group(walk(i, j), group);
    c.faces.push(Face::with_group(walk(j, i), group));
    Some(c.faces.len() - 1)
}

/// Turn edge `a`-`b` one step within its two faces: each end moves on to the
/// next corner of the polygon the two faces make together. RETURNS: false
/// when the edge doesn't have two faces or the turn would make an edge that
/// already exists.
pub fn spin_edge(c: &mut Cage, a: u32, b: u32) -> bool {
    let de = directed_edges(c);
    let (Some(&f), Some(&g)) = (de.get(&(a, b)), de.get(&(b, a))) else { return false };
    if f == g {
        return false;
    }
    // f runs a -> b; g runs b -> a. The joint polygon from b: f from b round
    // to a, then g from a round to b (without the ends).
    let walk = |face: usize, start: u32, end: u32| -> Vec<u32> {
        let k = corners(&c.faces[face]);
        let i = k.iter().position(|&v| v == start).expect("corner");
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
    let mut ring = walk(f, b, a);
    let ia = ring.len() - 1;
    let tail = walk(g, a, b);
    ring.extend(&tail[1..tail.len() - 1]);
    let n = ring.len();
    if n < 4 || ring.iter().collect::<HashSet<_>>().len() != n {
        return false;
    }
    // Ends move on one corner each: b (at 0) to 1, a (at ia) to ia + 1.
    let (x, y) = (1, (ia + 1) % n);
    if x == y || (y + 1) % n == x || (x + 1) % n == y {
        return false;
    }
    let (p, q) = (ring[x], ring[y]);
    if c.edges().contains(&edge_key(p, q)) {
        return false;
    }
    let part = |from: usize, to: usize| -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = from;
        loop {
            out.push(ring[i]);
            if i == to {
                break;
            }
            i = (i + 1) % n;
        }
        out
    };
    let (gf, gg) = (c.faces[f].group, c.faces[g].group);
    c.faces[f] = Face::with_group(part(x, y), gf);
    c.faces[g] = Face::with_group(part(y, x), gg);
    true
}

/// Slide the points of `edges` along the edges leaving them: a positive
/// amount moves them towards one side, a negative one towards the other; 1
/// reaches the next point. The edges are joined into chains (or loops) and
/// each chain is given a direction: with consistent winding, the face that
/// runs a chain's edges forwards is always on the same side of it, so that
/// side is "positive" all along. The chain through the first edge goes
/// towards the side of the face running it forwards.
///
/// RETURNS: the slide, with `back` set.
pub fn edge_slide(c: &Cage, edges: &[[u32; 2]]) -> Slide {
    let de = directed_edges(c);
    let mut set: Vec<[u32; 2]> = edges.iter().map(|&[a, b]| edge_key(a, b)).collect();
    set.sort_unstable();
    set.dedup();
    let in_set: HashSet<[u32; 2]> = set.iter().copied().collect();
    let mut at_point: HashMap<u32, Vec<u32>> = HashMap::new();
    for &[a, b] in &set {
        at_point.entry(a).or_default().push(b);
        at_point.entry(b).or_default().push(a);
    }
    // Directed chain edges, each chain walked from one end (or anywhere on a loop).
    let mut directed: Vec<(u32, u32)> = Vec::new();
    let mut done: HashSet<[u32; 2]> = HashSet::new();
    let first = edges.first().map(|&[a, b]| (a, b));
    let mut starts: Vec<(u32, u32)> = first.into_iter().collect();
    starts.extend(set.iter().map(|&[a, b]| (a, b)));
    for (a, b) in starts {
        if done.contains(&edge_key(a, b)) {
            continue;
        }
        // Walk back from a to the chain's start, then forwards.
        let (mut u, mut v) = (b, a);
        let mut guard = 0;
        loop {
            let next = at_point[&v].iter().copied().filter(|&w| w != u).collect::<Vec<_>>();
            if at_point[&v].len() != 2 || next.len() != 1 || next[0] == b && v == a && guard > 0 {
                break;
            }
            if edge_key(v, next[0]) == edge_key(a, b) || guard > set.len() {
                break;
            }
            u = v;
            v = next[0];
            guard += 1;
        }
        // Now v is the start, u the point after it (towards a, b).
        let (mut from, mut to) = (v, u);
        if guard == 0 {
            (from, to) = (a, b);
        }
        let mut steps = 0;
        while done.insert(edge_key(from, to)) {
            directed.push((from, to));
            steps += 1;
            if at_point[&to].len() != 2 || steps > set.len() {
                break;
            }
            let Some(&next) = at_point[&to].iter().find(|&&w| w != from) else { break };
            from = to;
            to = next;
        }
    }
    // Keep the first edge's direction as given.
    if let Some((a, b)) = first
        && directed.iter().any(|&(x, y)| (x, y) == (b, a))
    {
        // The chain through it runs the other way: flip that chain's sides
        // by flipping every edge of it (chains are separate sets of points).
        let mut chain: HashSet<u32> = HashSet::from([a, b]);
        let mut grew = true;
        while grew {
            grew = false;
            for &(x, y) in &directed {
                if chain.contains(&x) != chain.contains(&y) {
                    chain.insert(x);
                    chain.insert(y);
                    grew = true;
                }
            }
        }
        for e in directed.iter_mut() {
            if chain.contains(&e.0) {
                *e = (e.1, e.0);
            }
        }
    }
    // Each point's direction on each side: towards its neighbour, in that
    // side's face, that isn't along the chain.
    let mut dir: HashMap<u32, [(Vec3, u32); 2]> = HashMap::new();
    for &(a, b) in &directed {
        for (which, face) in [(0, de.get(&(a, b))), (1, de.get(&(b, a)))] {
            let Some(&f) = face else { continue };
            for v in [a, b] {
                let Some((p, n)) = around(c, f, v) else { continue };
                let along = if in_set.contains(&edge_key(v, p)) { n } else { p };
                if in_set.contains(&edge_key(v, along)) {
                    continue;
                }
                let e = &mut dir.entry(v).or_default()[which];
                e.0 += c.positions[along as usize] - c.positions[v as usize];
                e.1 += 1;
            }
        }
    }
    let mut points: Vec<u32> = dir.keys().copied().collect();
    points.sort_unstable();
    let mut slide = Slide::default();
    for v in points {
        let [(d1, n1), (d2, n2)] = dir[&v];
        let avg = |d: Vec3, n: u32| if n > 0 { d / n as f32 } else { Vec3::ZERO };
        slide.push(c, v, avg(d1, n1));
        slide.back.push(avg(d2, n2));
    }
    slide
}

/// Even out the spacing of `points`: each moves `amount` (0 to 1) of the
/// way towards the middle of its neighbours, but only along the surface
/// (the part along its normal is dropped), so the shape keeps its volume.
/// Border points stay. RETURNS: the points that moved.
pub fn equalize(c: &mut Cage, points: &[u32], amount: f32) -> Vec<u32> {
    let amount = amount.clamp(0.0, 1.0);
    let edge_faces = c.edge_faces();
    let mut border = vec![false; c.positions.len()];
    let mut near: Vec<Vec<u32>> = vec![Vec::new(); c.positions.len()];
    for (&[a, b], fs) in &edge_faces {
        if fs.len() == 1 {
            border[a as usize] = true;
            border[b as usize] = true;
        }
        near[a as usize].push(b);
        near[b as usize].push(a);
    }
    let mut normal = vec![Vec3::ZERO; c.positions.len()];
    for f in 0..c.faces.len() {
        let n = face_normal(c, f);
        for &v in corners(&c.faces[f]) {
            normal[v as usize] += n;
        }
    }
    let moves: Vec<(u32, Vec3)> = points
        .iter()
        .copied()
        .filter(|&v| (v as usize) < c.positions.len() && !border[v as usize] && !near[v as usize].is_empty())
        .filter(|&v| !c.is_pinned(v))
        .map(|v| {
            let at = c.positions[v as usize];
            let ns = &near[v as usize];
            let mid = ns.iter().map(|&w| c.positions[w as usize]).sum::<Vec3>() / ns.len() as f32;
            let n = normal[v as usize].normalize_or_zero();
            let d = mid - at;
            (v, at + (d - n * n.dot(d)) * amount)
        })
        .collect();
    for &(v, p) in &moves {
        c.positions[v as usize] = p;
    }
    moves.into_iter().map(|(v, _)| v).collect()
}

/// Drop unused points after an edit (re-exported for the app).
pub fn tidy(c: &mut Cage) {
    remove_unused_points(c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyedit::validate;

    /// A unit cube as six quads, outward.
    fn cube() -> Cage {
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let positions = vec![
            p(-0.5, -0.5, -0.5),
            p(0.5, -0.5, -0.5),
            p(0.5, 0.5, -0.5),
            p(-0.5, 0.5, -0.5),
            p(-0.5, -0.5, 0.5),
            p(0.5, -0.5, 0.5),
            p(0.5, 0.5, 0.5),
            p(-0.5, 0.5, 0.5),
        ];
        let faces = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]]
            .into_iter()
            .map(|k| Face::new(k.to_vec()))
            .collect();
        Cage { positions, faces, linked: None, pinned: Vec::new() }
    }

    /// An `n` × `n` grid of quads in the XZ plane facing +Y.
    fn grid(n: u32) -> Cage {
        let mut positions = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                positions.push(Vec3::new(i as f32, 0.0, -(j as f32)));
            }
        }
        let at = |i: u32, j: u32| j * (n + 1) + i;
        let mut faces = Vec::new();
        for j in 0..n {
            for i in 0..n {
                faces.push(Face::new(vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]));
            }
        }
        Cage { positions, faces, linked: None, pinned: Vec::new() }
    }

    fn euler(c: &Cage) -> i64 {
        let used = c.used().iter().filter(|u| **u).count() as i64;
        used - c.edges().len() as i64 + c.faces.len() as i64
    }

    #[test]
    fn bevel_one_cube_edge() {
        let mut c = cube();
        let (slide, made) = bevel_edges(&mut c, &[[2, 6]]);
        validate(&c).unwrap();
        assert_eq!(made.len(), 1, "one strip");
        assert_eq!(c.faces.len(), 7);
        // Both ends have three edges: each splits into two new points.
        assert_eq!(slide.verts.len(), 4);
        assert_eq!(c.used().iter().filter(|u| **u).count(), 10, "the two old corners are gone");
        assert_eq!(euler(&c), 2);
        slide.apply(&mut c, 0.1);
        // The new points sit 0.1 from the old corner along the cube's edges.
        let corner = Vec3::new(0.5, 0.5, -0.5);
        let used = c.used();
        let near: Vec<f32> = c
            .positions
            .iter()
            .zip(&used)
            .filter(|(_, u)| **u)
            .map(|(p, _)| p.distance(corner))
            .filter(|d| *d < 0.2)
            .collect();
        assert_eq!(near.len(), 2);
        assert!(near.iter().all(|d| (d - 0.1).abs() < 1e-5));
    }

    #[test]
    fn bevel_every_cube_edge_makes_corner_faces() {
        let mut c = cube();
        let edges = c.edges();
        let (slide, made) = bevel_edges(&mut c, &edges);
        validate(&c).unwrap();
        // 12 strips and 8 corner triangles; each corner splits into 3.
        assert_eq!(made.len(), 20);
        assert_eq!(c.faces.len(), 26);
        assert_eq!(slide.verts.len(), 24);
        assert_eq!(euler(&c), 2);
        assert_eq!(c.faces.iter().filter(|f| f.len() == 3).count(), 8);
    }

    #[test]
    fn bevel_an_edge_loop_on_a_grid() {
        let mut c = grid(3);
        // The middle row of vertical edges... take the line x = 1, all along z.
        let at = |i: u32, j: u32| j * 4 + i;
        let edges: Vec<[u32; 2]> = (0..3).map(|j| [at(1, j), at(1, j + 1)]).collect();
        let (slide, made) = bevel_edges(&mut c, &edges);
        validate(&c).unwrap();
        assert_eq!(made.len(), 3);
        // Interior points split in two; border ends too (one free edge on each side).
        assert_eq!(slide.verts.len(), 8);
        slide.apply(&mut c, 0.25);
        validate(&c).unwrap();
        tidy(&mut c);
        assert_eq!(euler(&c), 1, "still a disc");
    }

    #[test]
    fn bevel_a_single_inner_edge_keeps_its_four_edge_ends() {
        let mut c = grid(3);
        let at = |i: u32, j: u32| j * 4 + i;
        let (slide, made) = bevel_edges(&mut c, &[[at(1, 1), at(2, 1)]]);
        validate(&c).unwrap();
        assert_eq!(made.len(), 1);
        // A hexagon: two new points and the old one at each end.
        assert_eq!(c.faces[made[0]].len(), 6);
        assert_eq!(slide.verts.len(), 4);
        slide.apply(&mut c, 0.3);
        validate(&c).unwrap();
        tidy(&mut c);
        assert_eq!(euler(&c), 1);
    }

    #[test]
    fn bevel_faces_moves_the_cap_out_and_in() {
        let mut c = cube();
        let (slide, made) = bevel_faces(&mut c, &[1], false);
        validate(&c).unwrap();
        assert_eq!(made.len(), 4);
        slide.apply(&mut c, 0.1);
        let cap: Vec<Vec3> = corners(&c.faces[1]).iter().map(|&v| c.positions[v as usize]).collect();
        for p in cap {
            assert!((p.z - 0.6).abs() < 1e-5, "out by 0.1");
            assert!((p.x.abs() - 0.4).abs() < 1e-5 && (p.y.abs() - 0.4).abs() < 1e-5, "in by 0.1: {p}");
        }
    }

    #[test]
    fn bridge_two_cube_faces_makes_a_hole_through() {
        let mut c = cube();
        let made = bridge_faces(&mut c, 0, 1, 2, 0).unwrap();
        validate(&c).unwrap();
        assert_eq!(made.len(), 8, "4 sides × 2 rows");
        assert_eq!(c.faces.len(), 12);
        // A torus: V - E + F = 0.
        assert_eq!(euler(&c), 0);
        // No twist: tube edges run straight along Z.
        for f in &made {
            let k = corners(&c.faces[*f]);
            let zs: HashSet<i32> = k.iter().map(|&v| (c.positions[v as usize].z * 100.0).round() as i32).collect();
            assert_eq!(zs.len(), 2);
        }
        // Either order, and with a twist.
        for (a, b, twist) in [(1, 0, 0), (0, 1, 1), (1, 0, -1)] {
            let mut d = cube();
            bridge_faces(&mut d, a, b, 3, twist).unwrap();
            validate(&d).unwrap();
        }
        let mut d = cube();
        assert!(bridge_faces(&mut d, 0, 2, 1, 0).is_err(), "touching faces");
        // On a cube the corners of opposite faces are joined already: one row
        // would put faces on those edges twice.
        let mut d = cube();
        assert!(bridge_faces(&mut d, 0, 1, 1, 0).is_err());
    }

    #[test]
    fn split_and_spin() {
        // The quad is 0, 1, 3, 2.
        let mut c = grid(1);
        assert!(split_face(&mut c, 0, 1).is_none(), "next to each other");
        let f2 = split_face(&mut c, 0, 3).unwrap();
        validate(&c).unwrap();
        assert_eq!(c.faces.len(), 2);
        assert_eq!(c.faces[f2].len(), 3);
        assert!(c.edges().contains(&[0, 3]));
        // Spin the new diagonal: it becomes the other one.
        assert!(spin_edge(&mut c, 0, 3));
        validate(&c).unwrap();
        assert!(c.edges().contains(&[1, 2]));
        assert!(!c.edges().contains(&[0, 3]));
        // A border edge can't spin.
        assert!(!spin_edge(&mut c, 0, 1));
        // On a 2 × 1 grid the shared edge spins into a diagonal of the pair.
        let mut g = grid(2);
        assert!(spin_edge(&mut g, 1, 4));
        validate(&g).unwrap();
        assert!(!g.edges().contains(&[1, 4]));
    }

    #[test]
    fn edge_slide_moves_a_loop_to_either_side() {
        let mut c = grid(2);
        let at = |i: u32, j: u32| j * 3 + i;
        let edges = vec![[at(1, 0), at(1, 1)], [at(1, 1), at(1, 2)]];
        let slide = edge_slide(&c, &edges);
        assert_eq!(slide.verts.len(), 3);
        slide.apply(&mut c, 0.5);
        let xs: Vec<f32> = slide.verts.iter().map(|&v| c.positions[v as usize].x).collect();
        assert!(xs.iter().all(|x| (x - xs[0]).abs() < 1e-5), "the loop stays straight: {xs:?}");
        assert!((xs[0] - 1.0).abs() > 0.4, "moved half way to one side");
        slide.apply(&mut c, -0.5);
        let back: Vec<f32> = slide.verts.iter().map(|&v| c.positions[v as usize].x).collect();
        assert!((back[0] - 1.0).abs() > 0.4 && (back[0] - xs[0]).abs() > 0.9, "and to the other: {back:?}");
        validate(&c).unwrap();
    }

    #[test]
    fn equalize_spreads_points_along_the_surface() {
        let mut c = grid(2);
        let mid = 4;
        c.positions[mid as usize] += Vec3::new(0.4, 0.3, -0.2);
        let moved = equalize(&mut c, &[mid], 1.0);
        assert_eq!(moved, vec![mid]);
        let p = c.positions[mid as usize];
        // Back over the middle, and the bump stays (only moves along the surface).
        assert!((p.x - 1.0).abs() < 0.15 && (p.z + 1.0).abs() < 0.15, "{p}");
        assert!(p.y > 0.1, "kept off the plane: {p}");
    }
}
