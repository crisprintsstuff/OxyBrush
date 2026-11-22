//! OxySpheres (RFC 0034): a tree of spheres that blocks out a shape. Each
//! sphere is a joint; each link between a sphere and its parent is the
//! smooth, tapering envelope of all the spheres between the two ends
//! (a "round cone"). [`Armature::distance`] is the signed distance to that
//! shape, which `oxy_topo::skin_armature` turns into a sculptable mesh.
//!
//! Mirroring: with [`Armature::mirror`] set, spheres come in pairs across
//! the plane `x = mirror` (each knows its partner as `twin`) or sit on the
//! plane (no twin). Adding, moving, resizing or deleting one also does the
//! mirrored edit on its twin, so the armature stays symmetric.
//!
//! INVARIANT: index 0 is the root (no parent); every other sphere has a
//! parent, and following parents from any sphere reaches the root (no
//! cycles). [`Armature::is_valid`] checks this for loaded data.

use crate::{Aabb, Mesh, MeshData};
use glam::Vec3;

/// A sphere of the tree: a joint of the shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
    /// The sphere this one hangs from; None only for the root (index 0).
    pub parent: Option<u32>,
    /// Its mirror partner across the X plane, while mirroring.
    pub twin: Option<u32>,
}

/// A tree of spheres.
#[derive(Debug, Clone, PartialEq)]
pub struct Armature {
    pub spheres: Vec<Sphere>,
    /// The X of the mirror plane; None: no mirroring.
    pub mirror: Option<f32>,
}

/// Smallest radius a sphere can have.
pub const MIN_RADIUS: f32 = 1e-4;

impl Armature {
    /// One root sphere.
    pub fn new(center: Vec3, radius: f32, mirror: Option<f32>) -> Self {
        let center = match mirror {
            Some(m) => Vec3::new(m, center.y, center.z),
            None => center,
        };
        Armature { spheres: vec![Sphere { center, radius: radius.max(MIN_RADIUS), parent: None, twin: None }], mirror }
    }

    pub fn len(&self) -> usize {
        self.spheres.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spheres.is_empty()
    }

    /// Whether the tree is well formed (see the module's INVARIANT) and
    /// every sphere is finite with a positive radius.
    pub fn is_valid(&self) -> bool {
        let n = self.spheres.len();
        if n == 0 || self.spheres[0].parent.is_some() {
            return false;
        }
        self.spheres.iter().enumerate().all(|(i, s)| {
            if !(s.center.is_finite() && s.radius.is_finite() && s.radius > 0.0) {
                return false;
            }
            if s.twin.is_some_and(|t| t as usize >= n || t as usize == i) {
                return false;
            }
            if i > 0 && s.parent.is_none() {
                return false;
            }
            // Walk up: the root within n steps.
            let mut at = i;
            for _ in 0..=n {
                match self.spheres[at].parent {
                    None => return at == 0,
                    Some(p) if (p as usize) < n => at = p as usize,
                    Some(_) => return false,
                }
            }
            false
        })
    }

    /// The links of the tree: (child, parent) for every sphere but the root.
    pub fn links(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.spheres.iter().enumerate().filter_map(|(i, s)| s.parent.map(|p| (i, p as usize)))
    }

    pub fn children(&self, i: usize) -> Vec<usize> {
        self.links().filter(|&(_, p)| p == i).map(|(c, _)| c).collect()
    }

    /// `i` and every sphere below it.
    pub fn branch(&self, i: usize) -> Vec<usize> {
        let mut out = vec![i];
        let mut k = 0;
        while k < out.len() {
            let at = out[k];
            out.extend(self.children(at));
            k += 1;
        }
        out
    }

    fn mirrored(&self, p: Vec3) -> Vec3 {
        match self.mirror {
            Some(m) => Vec3::new(2.0 * m - p.x, p.y, p.z),
            None => p,
        }
    }

    /// Whether sphere `i` sits on the mirror plane (it has no twin there).
    pub fn on_plane(&self, i: usize) -> bool {
        match self.mirror {
            Some(m) => {
                let s = &self.spheres[i];
                s.twin.is_none() && (s.center.x - m).abs() <= 1e-4 * s.radius.max(1.0)
            }
            None => false,
        }
    }

    fn push(&mut self, center: Vec3, radius: f32, parent: usize) -> usize {
        self.spheres.push(Sphere { center, radius: radius.max(MIN_RADIUS), parent: Some(parent as u32), twin: None });
        self.spheres.len() - 1
    }

    fn pair(&mut self, a: usize, b: usize) {
        self.spheres[a].twin = Some(b as u32);
        self.spheres[b].twin = Some(a as u32);
    }

    /// Add a sphere hanging from `parent` at `center`, and its twin while
    /// mirroring. A child of a sphere on the plane that is close to the
    /// plane (within a quarter of its radius) snaps onto it.
    ///
    /// RETURNS: the new sphere's index (not its twin's).
    pub fn add_child(&mut self, parent: usize, center: Vec3, radius: f32) -> usize {
        let Some(m) = self.mirror else { return self.push(center, radius, parent) };
        if let Some(tp) = self.spheres[parent].twin {
            let i = self.push(center, radius, parent);
            let t = self.push(self.mirrored(center), radius, tp as usize);
            self.pair(i, t);
            return i;
        }
        if self.on_plane(parent) && (center.x - m).abs() < 0.25 * radius {
            return self.push(Vec3::new(m, center.y, center.z), radius, parent);
        }
        let i = self.push(center, radius, parent);
        if self.on_plane(parent) {
            let t = self.push(self.mirrored(center), radius, parent);
            self.pair(i, t);
        }
        i
    }

    /// Put a sphere on the link from `child` up to its parent, `t` of the
    /// way from the parent (0) to the child (1), and on its twin's link.
    ///
    /// RETURNS: the new sphere's index; None for the root.
    pub fn insert_between(&mut self, child: usize, t: f32) -> Option<usize> {
        let p = self.spheres[child].parent? as usize;
        let t = t.clamp(0.0, 1.0);
        let make = |a: &mut Armature, child: usize, p: usize| {
            let (sp, sc) = (a.spheres[p], a.spheres[child]);
            let i = a.push(sp.center.lerp(sc.center, t), sp.radius + (sc.radius - sp.radius) * t, p);
            a.spheres[child].parent = Some(i as u32);
            i
        };
        let i = make(self, child, p);
        if let Some(tc) = self.spheres[child].twin.map(|t| t as usize)
            && let Some(tp) = self.spheres[tc].parent.map(|p| p as usize)
            && tp != i
        {
            let j = make(self, tc, tp);
            self.pair(i, j);
        }
        Some(i)
    }

    /// Move sphere `i` to `center` (its twin to the mirror image). A sphere
    /// on the plane stays on it. With `follow`, everything below it moves
    /// along by the same amount.
    pub fn move_to(&mut self, i: usize, center: Vec3, follow: bool) {
        let mut delta = center - self.spheres[i].center;
        if self.on_plane(i) {
            delta.x = 0.0;
        }
        let mirrored = Vec3::new(-delta.x, delta.y, delta.z);
        let twin = self.spheres[i].twin.map(|t| t as usize);
        let set = |a: &Armature, j: usize| if follow { a.branch(j) } else { vec![j] };
        let mine = set(self, i);
        let theirs = twin.map(|t| set(self, t)).unwrap_or_default();
        for &j in &mine {
            self.spheres[j].center += delta;
        }
        for &j in theirs.iter().filter(|j| !mine.contains(j)) {
            self.spheres[j].center += mirrored;
        }
    }

    /// Set sphere `i`'s radius (and its twin's).
    pub fn set_radius(&mut self, i: usize, radius: f32) {
        let r = radius.max(MIN_RADIUS);
        self.spheres[i].radius = r;
        if let Some(t) = self.spheres[i].twin {
            self.spheres[t as usize].radius = r;
        }
    }

    /// Delete sphere `i` with everything below it, and its twin's branch.
    /// The root stays.
    ///
    /// RETURNS: whether anything was deleted.
    pub fn remove_branch(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.spheres.len() {
            return false;
        }
        let mut gone = vec![false; self.spheres.len()];
        for j in self.branch(i) {
            gone[j] = true;
        }
        if let Some(t) = self.spheres[i].twin.map(|t| t as usize).filter(|&t| t != 0) {
            for j in self.branch(t) {
                gone[j] = true;
            }
        }
        let mut map = vec![u32::MAX; self.spheres.len()];
        let mut kept = Vec::with_capacity(self.spheres.len());
        for (j, s) in self.spheres.iter().enumerate() {
            if !gone[j] {
                map[j] = kept.len() as u32;
                kept.push(*s);
            }
        }
        for s in &mut kept {
            // Parents of kept spheres are kept: the removed set holds whole branches.
            s.parent = s.parent.map(|p| map[p as usize]);
            s.twin = s.twin.map(|t| map[t as usize]).filter(|&t| t != u32::MAX);
        }
        self.spheres = kept;
        true
    }

    /// The box holding every sphere, grown by `pad`.
    pub fn bounds(&self, pad: f32) -> Aabb {
        let mut b = Aabb::EMPTY;
        for s in &self.spheres {
            b = b.union(&Aabb {
                min: s.center - Vec3::splat(s.radius + pad),
                max: s.center + Vec3::splat(s.radius + pad),
            });
        }
        b
    }

    /// The shapes making up the armature, each a round cone (a lone
    /// sphere is one with both ends the same): every link, and the root
    /// when it has no links.
    pub fn primitives(&self) -> Vec<RoundCone> {
        let mut out: Vec<RoundCone> = self
            .links()
            .map(|(c, p)| {
                let (a, b) = (self.spheres[p], self.spheres[c]);
                RoundCone { a: a.center, b: b.center, ra: a.radius, rb: b.radius }
            })
            .collect();
        if out.is_empty()
            && let Some(s) = self.spheres.first()
        {
            out.push(RoundCone { a: s.center, b: s.center, ra: s.radius, rb: s.radius });
        }
        out
    }

    /// Signed distance from `p` to the armature's shape (negative inside),
    /// the links joined with a rounded fillet `blend` wide (0: a sharp
    /// union). See [`smooth_min`].
    pub fn distance(&self, p: Vec3, blend: f32) -> f32 {
        distance_to(&self.primitives(), p, blend)
    }

    /// The armature as it looks while being edited: a sphere per joint
    /// and the tapered envelope of each link, as separate, overlapping
    /// shells (no fillets). Cheap enough to rebuild on every edit.
    pub fn preview(&self, segments: u32) -> Mesh {
        let seg = segments.max(6);
        let mut positions: Vec<Vec3> = Vec::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        let sphere = crate::primitives::uv_sphere(seg, seg / 2, 1.0);
        for s in &self.spheres {
            let off = positions.len() as u32;
            positions.extend(sphere.positions.iter().map(|&p| s.center + p * s.radius));
            triangles.extend(sphere.triangles.iter().map(|t| t.map(|v| v + off)));
        }
        for (c, p) in self.links() {
            let (a, b) = (self.spheres[p], self.spheres[c]);
            cone_band(&mut positions, &mut triangles, a.center, a.radius, b.center, b.radius, seg);
        }
        Mesh::from_data(MeshData { positions, triangles, ..Default::default() })
    }
}

/// The envelope of the spheres along a segment, their radius going from
/// `ra` at `a` to `rb` at `b`: two spheres joined by the cone that touches
/// both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundCone {
    pub a: Vec3,
    pub b: Vec3,
    pub ra: f32,
    pub rb: f32,
}

impl RoundCone {
    /// Signed distance from `p` (negative inside).
    ///
    /// Why this formula: the shape is the union of the spheres centred at
    /// c(t) = a + t(b - a) with radius r(t) = ra + t(rb - ra), t in [0, 1],
    /// so its distance is the minimum over t of f(t) = |p - c(t)| - r(t).
    /// In the plane of the axis, with x along it from `a`, y the distance
    /// from it and L the length, f(t) = sqrt((x - tL)² + y²) - r(t), which
    /// is convex in t. Setting f'(t) = 0 gives (x - tL)/|p - c(t)| = s with
    /// s = (ra - rb)/L, hence t = (x - s·y/sqrt(1 - s²))/L; clamped to
    /// [0, 1], that is the minimum. When |s| ≥ 1 one end sphere holds the
    /// other, f is monotonic, and the minimum is at an end.
    pub fn distance(&self, p: Vec3) -> f32 {
        let ab = self.b - self.a;
        let l = ab.length();
        let ap = p - self.a;
        if l < 1e-9 {
            return ap.length() - self.ra.max(self.rb);
        }
        let s = (self.ra - self.rb) / l;
        if s.abs() >= 1.0 {
            return (ap.length() - self.ra).min((p - self.b).length() - self.rb);
        }
        let u = ab / l;
        let x = ap.dot(u);
        let y = (ap - u * x).length();
        let t = ((x - s * y / (1.0 - s * s).sqrt()) / l).clamp(0.0, 1.0);
        (ap - ab * t).length() - (self.ra + (self.rb - self.ra) * t)
    }

    /// Its bounding box, grown by `pad`.
    pub fn bounds(&self, pad: f32) -> Aabb {
        let ea = Vec3::splat(self.ra + pad);
        let eb = Vec3::splat(self.rb + pad);
        Aabb { min: (self.a - ea).min(self.b - eb), max: (self.a + ea).max(self.b + eb) }
    }
}

/// Smooth minimum of two distances: `min(a, b)` with a rounded fillet
/// where they are within `k` of each other (the quadratic polynomial
/// smooth minimum; k = 0 is a plain min). It never exceeds `min(a, b)`,
/// and it is at most k/4 below it.
pub fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// Signed distance to the smooth union of `shapes`, in their order.
pub fn distance_to(shapes: &[RoundCone], p: Vec3, blend: f32) -> f32 {
    shapes
        .iter()
        .fold(f32::INFINITY, |d, c| if d.is_infinite() { c.distance(p) } else { smooth_min(d, c.distance(p), blend) })
}

/// The side of a round cone (the part of the cone that touches both end
/// spheres), as a band of quads between two rings.
fn cone_band(positions: &mut Vec<Vec3>, triangles: &mut Vec<[u32; 3]>, a: Vec3, ra: f32, b: Vec3, rb: f32, seg: u32) {
    let ab = b - a;
    let l = ab.length();
    if l < 1e-9 {
        return;
    }
    let s = (ra - rb) / l;
    if s.abs() >= 1.0 {
        return;
    }
    let u = ab / l;
    let c = (1.0 - s * s).sqrt();
    // The cone touches each sphere on the circle s·r along the axis from
    // its centre, c·r from the axis.
    let (ca, cb) = (a + u * (ra * s), b + u * (rb * s));
    let (wa, wb) = (ra * c, rb * c);
    let side = if u.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let e1 = u.cross(side).normalize();
    let e2 = u.cross(e1);
    let off = positions.len() as u32;
    for k in 0..seg {
        let ang = std::f32::consts::TAU * k as f32 / seg as f32;
        let d = e1 * ang.cos() + e2 * ang.sin();
        positions.push(ca + d * wa);
        positions.push(cb + d * wb);
    }
    for k in 0..seg {
        let n = (k + 1) % seg;
        let (a0, b0, a1, b1) = (off + 2 * k, off + 2 * k + 1, off + 2 * n, off + 2 * n + 1);
        triangles.push([a0, a1, b1]);
        triangles.push([a0, b1, b0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_cone_distance_matches_the_spheres_it_envelops() {
        let c = RoundCone { a: Vec3::ZERO, b: Vec3::new(2.0, 0.0, 0.0), ra: 0.5, rb: 0.25 };
        // On the axis past each end: the end sphere.
        assert!((c.distance(Vec3::new(-1.0, 0.0, 0.0)) - 0.5).abs() < 1e-5);
        assert!((c.distance(Vec3::new(3.0, 0.0, 0.0)) - 0.75).abs() < 1e-5);
        assert!(c.distance(Vec3::new(1.0, 0.0, 0.0)) < 0.0, "the middle is inside");
        // A brute force minimum over the swept spheres agrees everywhere.
        for i in 0..200 {
            let f = i as f32;
            let p = Vec3::new((f * 0.37).sin() * 3.0, (f * 0.91).cos() * 2.0, (f * 0.13).sin());
            let brute = (0..=4000)
                .map(|k| {
                    let t = k as f32 / 4000.0;
                    (p - c.a.lerp(c.b, t)).length() - (c.ra + (c.rb - c.ra) * t)
                })
                .fold(f32::INFINITY, f32::min);
            assert!((c.distance(p) - brute).abs() < 2e-3, "{p}: {} vs {brute}", c.distance(p));
        }
        // One end sphere holding the other: the bigger sphere.
        let held = RoundCone { a: Vec3::ZERO, b: Vec3::new(0.1, 0.0, 0.0), ra: 1.0, rb: 0.2 };
        assert!((held.distance(Vec3::new(0.0, 2.0, 0.0)) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn smooth_min_never_exceeds_the_min() {
        for (a, b) in [(0.0, 0.0), (1.0, 0.2), (-0.3, 0.1), (5.0, -5.0)] {
            let s = smooth_min(a, b, 0.5);
            assert!(s <= a.min(b) + 1e-6);
            assert!(s >= a.min(b) - 0.125 - 1e-6);
        }
        assert_eq!(smooth_min(1.0, 2.0, 0.0), 1.0);
    }

    fn mirrored() -> Armature {
        Armature::new(Vec3::new(0.3, 1.0, 0.0), 0.5, Some(0.0))
    }

    #[test]
    fn mirrored_children_come_in_pairs_and_near_the_plane_snap_onto_it() {
        let mut a = mirrored();
        assert_eq!(a.spheres[0].center.x, 0.0, "the root starts on the plane");
        let arm = a.add_child(0, Vec3::new(1.0, 1.0, 0.0), 0.3);
        assert_eq!(a.len(), 3);
        let twin = a.spheres[arm].twin.unwrap() as usize;
        assert_eq!(a.spheres[twin].center, Vec3::new(-1.0, 1.0, 0.0));
        assert_eq!(a.spheres[twin].twin, Some(arm as u32));
        // A child of an arm gets its twin on the other arm.
        let hand = a.add_child(arm, Vec3::new(2.0, 1.0, 0.0), 0.2);
        let hand_twin = a.spheres[hand].twin.unwrap() as usize;
        assert_eq!(a.spheres[hand_twin].parent, Some(twin as u32));
        // Near the plane from the root: one sphere, on the plane.
        let neck = a.add_child(0, Vec3::new(0.05, 2.0, 0.0), 0.3);
        assert_eq!(a.len(), 6);
        assert!(a.on_plane(neck));
        assert!(a.is_valid());
    }

    #[test]
    fn moving_resizing_and_deleting_follow_the_twin() {
        let mut a = mirrored();
        let arm = a.add_child(0, Vec3::new(1.0, 1.0, 0.0), 0.3);
        let hand = a.add_child(arm, Vec3::new(2.0, 1.0, 0.0), 0.2);
        let twin = a.spheres[arm].twin.unwrap() as usize;
        let hand_twin = a.spheres[hand].twin.unwrap() as usize;
        a.move_to(arm, Vec3::new(1.0, 1.5, 0.2), true);
        assert_eq!(a.spheres[twin].center, Vec3::new(-1.0, 1.5, 0.2));
        assert_eq!(a.spheres[hand].center, Vec3::new(2.0, 1.5, 0.2), "the branch follows");
        assert_eq!(a.spheres[hand_twin].center, Vec3::new(-2.0, 1.5, 0.2));
        a.move_to(arm, Vec3::new(1.2, 1.5, 0.2), false);
        assert_eq!(a.spheres[hand].center, Vec3::new(2.0, 1.5, 0.2), "alone, the branch stays");
        // The root on the plane slides along it only.
        a.move_to(0, Vec3::new(0.7, 0.0, 0.0), false);
        assert_eq!(a.spheres[0].center.x, 0.0);
        a.set_radius(hand, 0.4);
        assert_eq!(a.spheres[hand_twin].radius, 0.4);
        // Deleting an arm deletes both arms and hands.
        assert!(a.remove_branch(arm));
        assert_eq!(a.len(), 1);
        assert!(a.is_valid());
        assert!(!a.remove_branch(0), "the root stays");
    }

    #[test]
    fn inserting_on_a_link_splits_it_on_both_sides() {
        let mut a = mirrored();
        let arm = a.add_child(0, Vec3::new(2.0, 1.0, 0.0), 0.3);
        let twin = a.spheres[arm].twin.unwrap() as usize;
        let mid = a.insert_between(arm, 0.5).unwrap();
        assert_eq!(a.spheres[arm].parent, Some(mid as u32));
        assert_eq!(a.spheres[mid].parent, Some(0));
        assert!((a.spheres[mid].center - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-6);
        assert!((a.spheres[mid].radius - 0.4).abs() < 1e-6);
        let mid_twin = a.spheres[mid].twin.unwrap() as usize;
        assert_eq!(a.spheres[twin].parent, Some(mid_twin as u32));
        assert!(a.is_valid());
        assert_eq!(a.insert_between(0, 0.5), None);
    }

    #[test]
    fn validity_rejects_cycles_and_bad_spheres() {
        let mut a = Armature::new(Vec3::ZERO, 1.0, None);
        a.add_child(0, Vec3::X, 0.5);
        a.add_child(1, Vec3::X * 2.0, 0.5);
        assert!(a.is_valid());
        let mut cyc = a.clone();
        cyc.spheres[1].parent = Some(2);
        assert!(!cyc.is_valid());
        let mut bad = a.clone();
        bad.spheres[2].radius = 0.0;
        assert!(!bad.is_valid());
        let mut out = a;
        out.spheres[2].parent = Some(9);
        assert!(!out.is_valid());
    }

    #[test]
    fn the_preview_has_a_sphere_per_joint_and_a_band_per_link() {
        let mut a = Armature::new(Vec3::ZERO, 1.0, None);
        a.add_child(0, Vec3::new(3.0, 0.0, 0.0), 0.5);
        let p = a.preview(12);
        let sphere = crate::primitives::uv_sphere(12, 6, 1.0).triangle_count();
        assert_eq!(p.triangle_count(), 2 * sphere + 24);
        // The band touches the spheres: its points are on the shape's surface.
        let shape = a.primitives();
        let (mut band, mut tris) = (Vec::new(), Vec::new());
        cone_band(&mut band, &mut tris, Vec3::ZERO, 1.0, Vec3::new(3.0, 0.0, 0.0), 0.5, 12);
        assert_eq!(tris.len(), 24);
        let worst = band.iter().map(|&q| distance_to(&shape, q, 0.0).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-4, "{worst}");
    }
}
