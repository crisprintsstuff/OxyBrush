//! Smooth subdivision of a polygon [`Cage`]: Catmull–Clark with
//! semi-sharp creases (RFC 0018, phase C).
//!
//! One level turns every face with `n` corners into `n` quads, through a
//! point in the middle of the face, one on each edge and the old corners
//! moved. The rules are those of Catmull & Clark, "Recursively generated
//! B-spline surfaces on arbitrary topological meshes" (CAD, 1978), with
//! the crease rules of DeRose, Kass & Truong, "Subdivision surfaces in
//! character animation" (SIGGRAPH 1998):
//!
//! - **Face point:** the average of the face's corners.
//! - **Edge point:** smooth, `(a + b + f0 + f1) / 4`; on a sharp edge (a
//!   crease or a border) the edge's middle; a semi-sharp edge (sharpness
//!   `s` between 0 and 1) blends the two by `s`.
//! - **Corner (vertex) point**, by how many sharp edges meet there: fewer
//!   than two, the smooth rule `(Q + 2R + (n − 3) v) / n` (Q the average of
//!   the face points around it, R of its edges' middles, n its edge count);
//!   two, the crease rule `(a + 6 v + b) / 8` along them; more, it stays
//!   (a corner). The point's sharpness is the average of its sharp edges';
//!   below 1 the rule blends with the smooth one.
//!   A border point that belongs to one face only (the corner of an open
//!   sheet) stays where it is.
//! - **Sharpness** goes down by one per level, so a semi-sharp crease is
//!   sharp for a few levels and then smooths out, which rounds it a little.
//!
//! A crease **weight** (0 to 1, as the cage stores it) is sharpness
//! `4 × weight`; weight 1 is sharp at every level. Borders are always
//! sharp, so open surfaces keep their outline.

use crate::retopo::{Cage, Face, corners, edge_key};
use glam::Vec3;
use std::collections::HashMap;

/// Sharpness of a crease of weight 1 (and of borders): never runs out.
const SHARP: f32 = f32::INFINITY;
/// Sharpness per unit of crease weight below 1.
const PER_WEIGHT: f32 = 4.0;

fn sharpness(weight: f32) -> f32 {
    if weight >= 1.0 { SHARP } else { (weight * PER_WEIGHT).max(0.0) }
}

fn weight(sharpness: f32) -> f32 {
    if sharpness.is_infinite() { 1.0 } else { (sharpness / PER_WEIGHT).clamp(0.0, 1.0) }
}

/// One level of Catmull–Clark subdivision of `c`.
///
/// RETURNS: a cage of quads. Point `i` of `c` is still point `i` (moved);
/// the edge points follow, then the face points. Each quad keeps its face's
/// group, pinned points stay pinned, and creases carry on to the two halves
/// of each edge with one level less sharpness.
pub fn subdivide(c: &Cage) -> Cage {
    let n = c.positions.len();
    let pos = &c.positions;
    // Edges in first-seen order, with their faces.
    let mut edge_index: HashMap<[u32; 2], usize> = HashMap::new();
    let mut edges: Vec<([u32; 2], Vec<usize>)> = Vec::new();
    for (f, face) in c.faces.iter().enumerate() {
        let k = corners(face);
        for i in 0..k.len() {
            let key = edge_key(k[i], k[(i + 1) % k.len()]);
            let e = *edge_index.entry(key).or_insert_with(|| {
                edges.push((key, Vec::new()));
                edges.len() - 1
            });
            edges[e].1.push(f);
        }
    }
    let face_point: Vec<Vec3> = c
        .faces
        .iter()
        .map(|f| corners(f).iter().map(|&v| pos[v as usize]).sum::<Vec3>() / f.len().max(1) as f32)
        .collect();
    // An edge is sharp forever on a border (or with more than two faces).
    let edge_sharp: Vec<f32> =
        edges.iter().map(|(k, fs)| if fs.len() != 2 { SHARP } else { sharpness(c.crease(k[0], k[1])) }).collect();
    let edge_point: Vec<Vec3> = edges
        .iter()
        .zip(&edge_sharp)
        .map(|((k, fs), &s)| {
            let mid = (pos[k[0] as usize] + pos[k[1] as usize]) * 0.5;
            if s >= 1.0 {
                return mid;
            }
            let smooth = (pos[k[0] as usize] + pos[k[1] as usize] + face_point[fs[0]] + face_point[fs[1]]) * 0.25;
            smooth.lerp(mid, s)
        })
        .collect();
    // Around each point: its edges and faces.
    let mut point_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (e, (k, _)) in edges.iter().enumerate() {
        point_edges[k[0] as usize].push(e);
        point_edges[k[1] as usize].push(e);
    }
    let mut point_faces: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (f, face) in c.faces.iter().enumerate() {
        for &v in corners(face) {
            point_faces[v as usize].push(f);
        }
    }
    let mut positions: Vec<Vec3> = (0..n)
        .map(|v| {
            let p = pos[v];
            let (es, fs) = (&point_edges[v], &point_faces[v]);
            if es.is_empty() || fs.is_empty() {
                return p;
            }
            let valence = es.len() as f32;
            let q = fs.iter().map(|&f| face_point[f]).sum::<Vec3>() / fs.len() as f32;
            let r = es.iter().map(|&e| (pos[edges[e].0[0] as usize] + pos[edges[e].0[1] as usize]) * 0.5).sum::<Vec3>()
                / valence;
            let smooth = (q + 2.0 * r + (valence - 3.0) * p) / valence;
            let sharp: Vec<usize> = es.iter().copied().filter(|&e| edge_sharp[e] > 0.0).collect();
            if sharp.len() < 2 {
                return smooth;
            }
            let s = sharp.iter().map(|&e| edge_sharp[e]).sum::<f32>() / sharp.len() as f32;
            // A border point with one face is the corner of an open sheet:
            // it stays, so the sheet keeps its corners.
            let rule = if sharp.len() == 2 && fs.len() > 1 {
                let other = |e: usize| {
                    let k = edges[e].0;
                    pos[if k[0] as usize == v { k[1] } else { k[0] } as usize]
                };
                (other(sharp[0]) + 6.0 * p + other(sharp[1])) / 8.0
            } else {
                p
            };
            if s >= 1.0 { rule } else { smooth.lerp(rule, s) }
        })
        .collect();
    let edge_base = n as u32;
    positions.extend(&edge_point);
    let face_base = positions.len() as u32;
    positions.extend(&face_point);
    let ep = |a: u32, b: u32| edge_base + edge_index[&edge_key(a, b)] as u32;
    let mut faces = Vec::with_capacity(c.faces.iter().map(|f| f.len()).sum());
    for (f, face) in c.faces.iter().enumerate() {
        let k = corners(face);
        let m = k.len();
        for i in 0..m {
            let (prev, at, next) = (k[(i + m - 1) % m], k[i], k[(i + 1) % m]);
            faces.push(Face::with_group(vec![at, ep(at, next), face_base + f as u32, ep(prev, at)], face.group));
        }
    }
    let mut out = Cage { positions, faces, pinned: c.pinned.clone(), ..Default::default() };
    for (e, (k, fs)) in edges.iter().enumerate() {
        let s = edge_sharp[e];
        if fs.len() != 2 || s <= 0.0 {
            continue;
        }
        let next = s - 1.0;
        if next > 0.0 {
            let mid = edge_base + e as u32;
            out.set_crease(k[0], mid, weight(next));
            out.set_crease(mid, k[1], weight(next));
        }
    }
    out
}

/// `levels` levels of [`subdivide`].
pub fn subdivide_levels(c: &Cage, levels: u32) -> Cage {
    let mut out = c.clone();
    out.linked = None;
    for _ in 0..levels {
        out = subdivide(&out);
    }
    out
}

/// How many faces `levels` levels of subdivision make from `c`.
pub fn face_count(c: &Cage, levels: u32) -> usize {
    let first: usize = c.faces.iter().map(|f| f.len()).sum();
    if levels == 0 { c.faces.len() } else { first * 4usize.pow(levels - 1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyedit::validate;

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
            .map(|k| Face::with_group(k.to_vec(), 3))
            .collect();
        Cage { positions, faces, ..Default::default() }
    }

    fn euler(c: &Cage) -> i64 {
        c.used().iter().filter(|u| **u).count() as i64 - c.edges().len() as i64 + c.faces.len() as i64
    }

    #[test]
    fn a_cube_rounds_towards_a_ball() {
        let c = cube();
        let one = subdivide(&c);
        validate(&one).unwrap();
        assert_eq!(one.faces.len(), 24);
        assert_eq!(one.positions.len(), 8 + 12 + 6);
        assert_eq!(euler(&one), 2);
        assert!(one.faces.iter().all(|f| f.len() == 4 && f.group == 3), "quads, groups kept");
        // Known values: a corner moves to (±5/18)·(1,1,1)·... = 0.2778, a face
        // point stays at the face centre, an edge point to 0.375 out.
        let corner = one.positions[6];
        assert!((corner - Vec3::splat(5.0 / 18.0)).length() < 1e-5, "{corner}");
        let three = subdivide_levels(&c, 3);
        validate(&three).unwrap();
        assert_eq!(three.faces.len(), 6 * 64);
        assert_eq!(face_count(&c, 3), 6 * 64);
        let r: Vec<f32> = three.positions.iter().map(|p| p.length()).collect();
        let (lo, hi) = r.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        assert!(hi / lo < 1.25, "close to round: {lo}..{hi}");
    }

    #[test]
    fn creases_keep_edges_sharp_and_half_weight_is_in_between() {
        let mut c = cube();
        for [a, b] in c.edges() {
            c.set_crease(a, b, 1.0);
        }
        let sharp = subdivide_levels(&c, 3);
        validate(&sharp).unwrap();
        for p in &sharp.positions {
            assert!((p.abs().max_element() - 0.5).abs() < 1e-5, "stays on the cube: {p}");
        }
        assert!(sharp.creases.values().all(|&w| w == 1.0));
        // Corners stay put with three sharp edges.
        assert!((sharp.positions[6] - Vec3::splat(0.5)).length() < 1e-6);
        // Half weight: the corner ends up between the round and the sharp cube.
        let mut half = cube();
        for [a, b] in half.edges() {
            half.set_crease(a, b, 0.5);
        }
        let soft = subdivide_levels(&half, 3).positions[6].x;
        let round = subdivide_levels(&cube(), 3).positions[6].x;
        assert!(soft > round + 0.02 && soft < 0.5 - 0.01, "{round} < {soft} < 0.5");
    }

    #[test]
    fn a_creased_loop_stays_flat_and_borders_stay_put() {
        // Crease the four edges round the top face (y = 0.5): every point on
        // the loop has two sharp edges, so it follows the crease rule and the
        // loop stays in its plane, as a curve, while the rest rounds off.
        let mut c = cube();
        for (a, b) in [(2, 3), (3, 7), (7, 6), (6, 2)] {
            c.set_crease(a, b, 1.0);
        }
        let levels = 2;
        let s = subdivide_levels(&c, levels);
        validate(&s).unwrap();
        let flat = s.positions.iter().filter(|p| (p.y - 0.5).abs() < 1e-5).count();
        assert!(flat >= 4 * 2usize.pow(levels), "the loop's points stay at y = 0.5: {flat}");
        // The top corner is on the loop, pulled in along it (a curve, not a square).
        let corner = s.positions[6];
        assert!((corner.y - 0.5).abs() < 1e-5 && corner.x < 0.5 - 0.05, "{corner}");
        // Below the loop everything is rounded inwards.
        assert!(s.positions.iter().all(|p| p.y <= 0.5 + 1e-5));
        // An open grid keeps its border in its plane and its corners fixed.
        let mut g = Cage::default();
        for j in 0..3 {
            for i in 0..3 {
                g.positions.push(Vec3::new(i as f32, 0.0, -(j as f32)));
            }
        }
        let at = |i: u32, j: u32| j * 3 + i;
        for j in 0..2 {
            for i in 0..2 {
                g.faces.push(Face::new(vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]));
            }
        }
        g.positions[4].y = 1.0;
        let s = subdivide_levels(&g, 2);
        validate(&s).unwrap();
        assert_eq!(s.positions[0], Vec3::ZERO);
        for v in s.border_edges().into_iter().flatten() {
            assert!(s.positions[v as usize].y.abs() < 1e-6);
        }
    }

    #[test]
    fn creases_follow_edits() {
        // A loop cut across a creased edge keeps both halves creased, and
        // renumbering (dropping points) keeps creases on the same edges.
        let mut c = cube();
        // The loop through edge 0-1 crosses the edges parallel to it, 2-3 among them.
        c.set_crease(2, 3, 1.0);
        let made = c.insert_loop_ends(0, 1, 0.5);
        let on_creased = made.iter().find(|&&(_, a, b)| edge_key(a, b) == [2, 3]).map(|&(m, _, _)| m);
        let m = on_creased.expect("the loop crosses edge 2-3");
        assert_eq!((c.crease(2, m), c.crease(m, 3)), (1.0, 1.0));
        let p1 = c.positions[2];
        crate::polyedit::delete_points(&mut c, &[0]);
        let one = (0..c.positions.len() as u32).find(|&v| c.positions[v as usize] == p1).unwrap();
        assert!(c.live_creases().iter().any(|(e, _)| e.contains(&one)));
    }

    #[test]
    fn triangles_and_ngons_become_quads() {
        let c = Cage {
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y, Vec3::new(-0.5, 0.5, 0.0)],
            faces: vec![Face::new(vec![0, 1, 2]), Face::new(vec![0, 2, 3, 4])],
            ..Default::default()
        };
        let s = subdivide(&c);
        validate(&s).unwrap();
        assert_eq!(s.faces.len(), 7);
        assert!(s.faces.iter().all(|f| f.len() == 4));
    }
}
