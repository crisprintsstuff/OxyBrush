//! Headless polygon skin for OxySpheres (RFC 0038).

use glam::{Quat, Vec3};
use oxy_mesh::armature::{Armature, Sphere};
use oxy_mesh::retopo::{Cage, Face, edge_key};
use oxy_mesh::{Mesh, catmull_clark};
use std::collections::{BTreeSet, HashMap, HashSet};

const SIDES: usize = 8;
pub const MAX_QUAD_SUBDIVISION: u8 = 2;
const MAX_SUBDIVIDED_FACES: usize = 16_384;
/// Default Resolution keeps one ring per sphere (historical cage density).
pub const DEFAULT_QUAD_RESOLUTION: u32 = 160;
type Ring = [u32; SIDES];

/// Controls for [`quad_skin_armature`] / [`quad_skin_mesh`] ([RFC 0038](../../../docs/rfcs/0038-oxyspheres-quad-base-mesh.md)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadSkinOptions {
    /// Catmull–Clark levels after the cage (0..=[`MAX_QUAD_SUBDIVISION`]).
    pub subdivision: u8,
    /// Junction softness in 0..=1. `0.5` matches the previous fixed collar.
    pub blend: f32,
    /// Extra rings along long links when above [`DEFAULT_QUAD_RESOLUTION`].
    pub resolution: u32,
}

impl Default for QuadSkinOptions {
    fn default() -> Self {
        Self { subdivision: 0, blend: 0.5, resolution: DEFAULT_QUAD_RESOLUTION }
    }
}

#[derive(Clone, Copy)]
struct Section {
    ring: Ring,
    normal: Vec3,
    axis: Vec3,
    center: Vec3,
    radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadSkinError {
    InvalidArmature,
    UnsupportedBranching,
    CrowdedLinks,
    InvalidSurface,
    SubdivisionBudget,
}

impl std::fmt::Display for QuadSkinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidArmature => "The armature needs at least two valid, distinct joints",
            Self::UnsupportedBranching => "Quad skin supports up to three links at each joint",
            Self::CrowdedLinks => "The links are too short or close together for the quad skin",
            Self::InvalidSurface => "The quad skin would overlap or produce invalid faces",
            Self::SubdivisionBudget => {
                "The quad skin exceeds the subdivision face limit. Lower Subdivision and try again"
            }
        })
    }
}

impl std::error::Error for QuadSkinError {}

/// Build a closed polygon cage around chains and connected three-way junctions.
/// Use `Cage::build_mesh` to keep the polygon boundaries in the sculpt mesh.
pub fn quad_skin_armature(armature: &Armature, opts: QuadSkinOptions) -> Result<Cage, QuadSkinError> {
    let blend = if opts.blend.is_finite() { opts.blend.clamp(0.0, 1.0) } else { 0.5 };
    let resolution = opts.resolution.max(32);
    use QuadSkinError::*;
    if !armature.is_valid() || armature.len() < 2 {
        return Err(InvalidArmature);
    }
    let spheres = &armature.spheres;
    let mut neighbors = vec![Vec::new(); spheres.len()];
    for (child, parent) in armature.links() {
        let length = spheres[child].center.distance(spheres[parent].center);
        if !length.is_finite() || length <= 1e-6 {
            return Err(InvalidArmature);
        }
        neighbors[child].push(parent);
        neighbors[parent].push(child);
    }
    if neighbors.iter().any(|adjacent| adjacent.len() > 3) {
        return Err(UnsupportedBranching);
    }

    let mut cage = Cage::default();
    let mut ports = vec![Vec::new(); spheres.len()];
    for (joint, adjacent) in neighbors.iter().enumerate().filter(|(_, n)| n.len() == 3) {
        ports[joint] = junction_ports(&mut cage, spheres, spheres[joint], adjacent, blend)?;
    }
    let tree = SweepTree { spheres, neighbors: &neighbors, ports: &ports, resolution };
    if ports.iter().any(|rings| !rings.is_empty()) {
        // Mark the far collar so a path shared by two junctions is swept once.
        let mut used = HashSet::new();
        for (joint, rings) in ports.iter().enumerate() {
            for (&next, &port) in neighbors[joint].iter().zip(rings) {
                if !used.contains(&(joint, next))
                    && let Some(end) = sweep_path(&mut cage, &tree, joint, next, port)?
                {
                    used.insert(end);
                }
            }
        }
    } else {
        let start = neighbors.iter().position(|n| n.len() == 1).ok_or(InvalidArmature)?;
        let next = neighbors[start][0];
        let sphere = spheres[start];
        let direction = (spheres[next].center - sphere.center).normalize();
        let axis = direction.any_orthonormal_vector();
        let ring = add_ring(&mut cage, sphere.center, sphere.radius, direction, axis);
        cap(&mut cage, ring.ring, sphere.center - direction * sphere.radius, true);
        sweep_path(&mut cage, &tree, start, next, ring)?;
    }
    validate(&cage)?;
    Ok(cage)
}

/// Build the polygon mesh at the requested detail, preserving the cage's faces.
pub fn quad_skin_mesh(armature: &Armature, opts: QuadSkinOptions) -> Result<Mesh, QuadSkinError> {
    let levels = opts.subdivision;
    if levels > MAX_QUAD_SUBDIVISION {
        return Err(QuadSkinError::SubdivisionBudget);
    }
    let mut cage = quad_skin_armature(armature, opts)?;
    if levels > 0 {
        if catmull_clark::face_count(&cage, u32::from(levels)) > MAX_SUBDIVIDED_FACES {
            return Err(QuadSkinError::SubdivisionBudget);
        }
        for _ in 0..levels {
            cage = catmull_clark::subdivide(&cage);
        }
        validate(&cage)?;
    }
    if let Some(mirror_x) = armature.mirror {
        // The cage triangulates opposite corners 0 and 2. Reflected
        // faces may start at different corners, so choose the start in
        // reflected coordinates to keep their shading diagonals paired.
        let positions = &cage.positions;
        for face in &mut cage.faces {
            if face.corners.len() != 4 {
                continue;
            }
            let key = |i: usize| {
                let p = positions[face.corners[i] as usize];
                [p.y, p.z, (p.x - mirror_x).abs()].map(|x| (x as f64 * 1.0e5).round() as i64)
            };
            let first = (0..4).min_by_key(|&i| key(i)).unwrap();
            face.corners.rotate_left(first);
        }
    }
    cage.build_mesh().ok_or(QuadSkinError::InvalidSurface)
}

fn junction_ports(
    cage: &mut Cage,
    spheres: &[Sphere],
    origin: Sphere,
    branches: &[usize],
    blend: f32,
) -> Result<Vec<Section>, QuadSkinError> {
    use QuadSkinError::CrowdedLinks;
    let directions: Vec<_> = branches.iter().map(|&i| (spheres[i].center - origin.center).normalize()).collect();
    let basis = (directions[1] - directions[0]).cross(directions[2] - directions[0]).normalize_or_zero();
    if basis == Vec3::ZERO {
        return Err(CrowdedLinks);
    }
    let mut ports = Vec::with_capacity(3);
    for (branch, (&next, &direction)) in branches.iter().zip(&directions).enumerate() {
        let angle = directions
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != branch)
            .map(|(_, &other)| direction.angle_between(other))
            .fold(f32::INFINITY, f32::min);
        let length = origin.center.distance(spheres[next].center);
        // Blend spreads the collar along the link, up to the room available.
        let base = 0.35 + 1.15 / (angle * 0.5).tan();
        let offset = origin.radius * base * (0.75 + blend * 0.5);
        let max_offset = length * 0.64;
        if !offset.is_finite() || max_offset <= 1e-6 {
            return Err(CrowdedLinks);
        }
        let offset = offset.min(max_offset);
        let center = origin.center + direction * offset;
        let radius = origin.radius + (spheres[next].radius - origin.radius) * (offset / length);
        let axis = (basis - direction * basis.dot(direction)).normalize_or_zero();
        if axis == Vec3::ZERO {
            return Err(CrowdedLinks);
        }
        ports.push(add_ring(cage, center, radius, direction, axis));
    }
    join_ports(cage, &ports, origin.center)?;
    Ok(ports)
}

fn add_ring(cage: &mut Cage, center: Vec3, radius: f32, direction: Vec3, axis: Vec3) -> Section {
    let tangent = direction.cross(axis);
    let ring = std::array::from_fn(|i| {
        let (sin, cos) = (std::f32::consts::TAU * i as f32 / SIDES as f32).sin_cos();
        let id = cage.positions.len() as u32;
        cage.positions.push(center + radius * (cos * axis + sin * tangent));
        id
    });
    Section { ring, normal: direction, axis, center, radius }
}

/// How many ring steps along a link. At [`DEFAULT_QUAD_RESOLUTION`] and below,
/// one ring per sphere (historical cages). Higher Resolution densifies long limbs.
fn link_steps(length: f32, radius: f32, resolution: u32) -> usize {
    if resolution <= DEFAULT_QUAD_RESOLUTION {
        return 1;
    }
    let spacing = radius.max(1e-3) * 2.0 * (DEFAULT_QUAD_RESOLUTION as f32 / resolution as f32);
    ((length / spacing.max(1e-3)).floor() as usize).clamp(1, 48)
}

fn cap(cage: &mut Cage, ring: Ring, tip: Vec3, reverse: bool) {
    let point = cage.positions.len() as u32;
    cage.positions.push(tip);
    for i in (0..SIDES).step_by(2) {
        // Start at the pole so the quad diagonal follows the cone.
        let mut corners = vec![point, ring[i], ring[(i + 1) % SIDES], ring[(i + 2) % SIDES]];
        if reverse {
            corners[1..].reverse();
        }
        cage.faces.push(Face::new(corners));
    }
}

struct SweepTree<'a> {
    spheres: &'a [Sphere],
    neighbors: &'a [Vec<usize>],
    ports: &'a [Vec<Section>],
    resolution: u32,
}

/// Sweep along a branch and return the far collar if the path joins two patches.
fn sweep_path(
    cage: &mut Cage,
    tree: &SweepTree<'_>,
    mut previous: usize,
    mut current: usize,
    mut section: Section,
) -> Result<Option<(usize, usize)>, QuadSkinError> {
    loop {
        let sphere = tree.spheres[current];
        let incoming = (sphere.center - tree.spheres[previous].center).normalize();
        if !tree.ports[current].is_empty() {
            let index = tree.neighbors[current].iter().position(|&n| n == previous).expect("adjacent joint");
            let end = tree.ports[current][index];
            // Each collar can fit its own junction and still overlap the other.
            if (end.center - section.center).dot(incoming) <= 1e-6 {
                return Err(QuadSkinError::CrowdedLinks);
            }
            join_collars(cage, section, end, tree.resolution);
            return Ok(Some((current, previous)));
        }
        let next = tree.neighbors[current].iter().copied().find(|&index| index != previous);
        let outgoing = next.map_or(incoming, |i| (tree.spheres[i].center - sphere.center).normalize());
        let direction = (incoming + outgoing).normalize_or_zero();
        if incoming.dot(outgoing) < -0.8 || section.normal.dot(direction) < -0.8 {
            return Err(QuadSkinError::CrowdedLinks);
        }
        let target_axis = Quat::from_rotation_arc(section.normal, direction) * section.axis;
        let length = section.center.distance(sphere.center);
        let avg_radius = (section.radius + sphere.radius) * 0.5;
        let steps = link_steps(length, avg_radius, tree.resolution);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let center = section.center.lerp(sphere.center, t);
            let radius = section.radius + (sphere.radius - section.radius) * t;
            let normal = (section.normal + (direction - section.normal) * t).normalize_or_zero();
            if normal == Vec3::ZERO {
                return Err(QuadSkinError::CrowdedLinks);
            }
            let axis = Quat::from_rotation_arc(section.normal, normal) * section.axis;
            let axis = if step == steps { target_axis } else { axis };
            let end = add_ring(cage, center, radius, if step == steps { direction } else { normal }, axis);
            bridge_rings(cage, section.ring, end.ring);
            section = end;
        }
        let Some(next) = next else {
            cap(cage, section.ring, sphere.center + direction * sphere.radius, false);
            return Ok(None);
        };
        (previous, current) = (current, next);
    }
}

fn join_collars(cage: &mut Cage, start: Section, end: Section, resolution: u32) {
    let transported = Quat::from_rotation_arc(start.normal, -end.normal) * start.axis;
    let offset = (0..SIDES)
        .max_by(|&a, &b| {
            let alignment = |i| (cage.positions[end.ring[i] as usize] - end.center).dot(transported);
            alignment(a).total_cmp(&alignment(b))
        })
        .unwrap();
    // The far collar faces the path. Reverse its winding and match the
    // nearest phase before bridging the existing rings.
    let aligned: Ring = std::array::from_fn(|i| end.ring[(offset + SIDES - i) % SIDES]);
    let steps = link_steps(start.center.distance(end.center), (start.radius + end.radius) * 0.5, resolution);
    let mut ring = start.ring;
    for step in 1..steps {
        let t = step as f32 / steps as f32;
        let next = std::array::from_fn(|i| {
            let p = cage.positions[start.ring[i] as usize].lerp(cage.positions[aligned[i] as usize], t);
            let id = cage.positions.len() as u32;
            cage.positions.push(p);
            id
        });
        bridge_rings(cage, ring, next);
        ring = next;
    }
    bridge_rings(cage, ring, aligned);
}

fn bridge_rings(cage: &mut Cage, start: Ring, end: Ring) {
    for i in 0..SIDES {
        let j = (i + 1) % SIDES;
        cage.faces.push(Face::new(vec![start[i], start[j], end[j], end[i]]));
    }
}

/// The convex hull of the three collar rings gives a closed junction whose
/// collar faces can be removed, leaving matching boundaries for the limbs.
/// The hull is small and fixed-size, so enumerate its supporting planes.
fn join_ports(cage: &mut Cage, ports: &[Section], origin: Vec3) -> Result<(), QuadSkinError> {
    let indices: Vec<_> = ports.iter().flat_map(|p| p.ring).collect();
    let points: Vec<_> = indices.iter().map(|&i| cage.positions[i as usize] - origin).collect();
    let tolerance = points.iter().map(|p| p.length()).fold(0.0, f32::max) * 1e-5;
    for Section { ring, normal, .. } in ports {
        let offset = normal.dot(cage.positions[ring[0] as usize] - origin);
        if points.iter().enumerate().any(|(i, p)| !ring.contains(&indices[i]) && p.dot(*normal) >= offset - tolerance) {
            return Err(QuadSkinError::CrowdedLinks);
        }
    }
    let mut seen = BTreeSet::new();
    for a in 0..points.len() {
        for b in a + 1..points.len() {
            for c in b + 1..points.len() {
                let mut n = (points[b] - points[a]).cross(points[c] - points[a]).normalize_or_zero();
                if n == Vec3::ZERO {
                    continue;
                }
                let mut distances: Vec<_> = points.iter().map(|&p| (p - points[a]).dot(n)).collect();
                let positive = distances.iter().any(|&d| d > tolerance);
                let negative = distances.iter().any(|&d| d < -tolerance);
                if positive == negative {
                    continue;
                }
                if positive {
                    n = -n;
                    distances.iter_mut().for_each(|d| *d = -*d);
                }
                let mut face: Vec<_> = (0..points.len()).filter(|&i| distances[i].abs() <= tolerance).collect();
                if ports.iter().any(|port| face.iter().all(|&i| port.ring.contains(&indices[i])))
                    || !seen.insert(face.clone())
                {
                    continue;
                }
                let center = face.iter().map(|&i| points[i]).sum::<Vec3>() / face.len() as f32;
                let u = (points[face[0]] - center).normalize();
                let v = n.cross(u);
                let angle = |i: usize| {
                    let radial = points[i] - center;
                    radial.dot(v).atan2(radial.dot(u))
                };
                face.sort_by(|&i, &j| angle(i).total_cmp(&angle(j)));
                // A center fan for larger facets avoids choosing a preferred
                // diagonal in a mirror-symmetric junction.
                if face.len() <= 4 {
                    cage.faces.push(Face::new(face.into_iter().map(|i| indices[i]).collect()));
                } else {
                    let tip = cage.positions.len() as u32;
                    cage.positions.push(origin + center);
                    for i in 0..face.len() {
                        cage.faces.push(Face::new(vec![indices[face[i]], indices[face[(i + 1) % face.len()]], tip]));
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate(cage: &Cage) -> Result<(), QuadSkinError> {
    let mut edges = HashMap::<[u32; 2], (usize, i32)>::new();
    for face in &cage.faces {
        for (&a, &b) in face.corners.iter().zip(face.corners.iter().cycle().skip(1)) {
            let (count, winding) = edges.entry(edge_key(a, b)).or_default();
            *count += 1;
            *winding += if a < b { 1 } else { -1 };
        }
    }
    if edges.values().any(|&(count, winding)| count != 2 || winding != 0)
        || cage.positions.len() + cage.faces.len() != edges.len() + 2
    {
        return Err(QuadSkinError::InvalidSurface);
    }

    let mut triangles = Vec::new();
    for ids in cage.triangulate().0 {
        let points = ids.map(|id| cage.positions[id as usize]);
        let normal = (points[1] - points[0]).cross(points[2] - points[0]);
        if !normal.is_finite() || normal.length_squared() == 0.0 {
            return Err(QuadSkinError::InvalidSurface);
        }
        let min = points[0].min(points[1]).min(points[2]);
        let max = points[0].max(points[1]).max(points[2]);
        triangles.push((ids, points, min, max));
    }
    // Sweep along the longest axis. A mostly vertical armature can have
    // overlapping X bounds along its entire length.
    let (low, high) = triangles
        .iter()
        .fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(low, high), (_, _, min, max)| {
            (low.min(*min), high.max(*max))
        });
    let span = high - low;
    let axis = if span.x >= span.y && span.x >= span.z {
        0
    } else if span.y >= span.z {
        1
    } else {
        2
    };
    triangles.sort_unstable_by(|a, b| a.2[axis].total_cmp(&b.2[axis]));
    for (i, (ids, a, min, max)) in triangles.iter().enumerate() {
        for (other, b, other_min, other_max) in triangles.iter().skip(i + 1).take_while(|t| t.2[axis] <= max[axis]) {
            if min.cmple(*other_max).all()
                && other_min.cmple(*max).all()
                && !ids.iter().any(|id| other.contains(id))
                && triangles_overlap(*a, *b)
            {
                return Err(QuadSkinError::InvalidSurface);
            }
        }
    }
    Ok(())
}

fn triangles_overlap(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    let edges = |p: [Vec3; 3]| [p[1] - p[0], p[2] - p[1], p[0] - p[2]];
    let (ea, eb) = (edges(a), edges(b));
    let (na, nb) = (ea[0].cross(ea[1]), eb[0].cross(eb[1]));
    let separated = |axis: Vec3| {
        let project = |points: [Vec3; 3]| {
            points
                .iter()
                .map(|&p| (p - a[0]).dot(axis))
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)))
        };
        let (lo_a, hi_a) = project(a);
        let (lo_b, hi_b) = project(b);
        hi_a < lo_b || hi_b < lo_a
    };
    !separated(na)
        && !separated(nb)
        && !ea.iter().any(|&x| eb.iter().any(|&y| separated(x.cross(y))))
        && !ea.iter().chain(&eb).any(|&edge| separated(na.cross(edge)) || separated(nb.cross(edge)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::catmull_clark::subdivide_levels;

    fn fork(width: f32, height: f32) -> Armature {
        let mut arm = Armature::new(Vec3::new(0.0, -3.0, 0.0), 0.34, Some(0.0));
        let joint = arm.add_child(0, Vec3::ZERO, 0.24);
        arm.add_child(joint, Vec3::new(width, height, 0.0), 0.11);
        assert_eq!(arm.len(), 4);
        arm
    }

    fn assert_subdivides(cage: &Cage) {
        let mut smooth = subdivide_levels(cage, 2);
        assert_eq!(smooth.faces.len(), smooth.quad_count());
        validate(&smooth).unwrap();
        let mesh = smooth.build_mesh().unwrap();
        assert!(crate::check_watertight(&mesh));
        assert!(mesh.known_polygons().is_some());
    }

    #[test]
    fn subdivided_quad_skin_keeps_polygon_faces_and_bounds_the_work() {
        let arm = fork(2.5, 1.0);
        let cage = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
        for level in 0..=MAX_QUAD_SUBDIVISION {
            let mesh = quad_skin_mesh(&arm, QuadSkinOptions { subdivision: level, ..Default::default() }).unwrap();
            assert!(crate::check_watertight(&mesh));
            let output = oxy_mesh::polyedit::cage_from_mesh(&mesh);
            assert_eq!(output.faces.len(), catmull_clark::face_count(&cage, u32::from(level)));
            if level > 0 {
                assert_eq!(output.faces.len(), output.quad_count());
                for &p in &output.positions {
                    let reflected = Vec3::new(-p.x, p.y, p.z);
                    assert!(output.positions.iter().any(|q| q.distance(reflected) < 2e-5));
                }
            }
        }
        let mut long_chain = Armature::new(Vec3::ZERO, 0.3, None);
        let mut end = 0;
        for i in 1..=128 {
            end = long_chain.add_child(end, Vec3::Y * (i as f32 * 2.0), 0.3);
        }
        assert!(matches!(
            quad_skin_mesh(&long_chain, QuadSkinOptions { subdivision: 2, ..Default::default() }),
            Err(QuadSkinError::SubdivisionBudget)
        ));
    }

    #[test]
    fn straight_and_bent_tapers_keep_quad_strips() {
        for bend in [0.0, 1.2] {
            let mut arm = Armature::new(Vec3::ZERO, 0.4, None);
            let middle = arm.add_child(0, Vec3::Y * 2.0, 0.3);
            arm.add_child(middle, Vec3::new(bend, 4.0, 0.0), 0.15);
            let cage = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
            assert_eq!(cage.quad_count(), 3 * SIDES);
            assert_eq!(cage.triangle_count(), 0);
            assert_subdivides(&cage);
        }
    }

    #[test]
    fn acute_and_wide_forks_are_deterministic_and_mirrored() {
        for (width, height) in [(0.8, 3.0), (2.5, 1.0)] {
            let arm = fork(width, height);
            let cage = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
            assert_eq!(cage, quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap());
            assert!(cage.quad_count() >= 3 * SIDES);
            assert!(cage.quad_count() > cage.triangle_count());
            let mirror_ids: Vec<_> = cage
                .positions
                .iter()
                .map(|p| {
                    let reflected = Vec3::new(-p.x, p.y, p.z);
                    cage.positions.iter().position(|q| q.distance(reflected) < 2e-5).unwrap() as u32
                })
                .collect();
            let face_keys: BTreeSet<_> = cage
                .faces
                .iter()
                .map(|f| {
                    let mut key = f.corners.clone();
                    key.sort_unstable();
                    key
                })
                .collect();
            for face in &cage.faces {
                let mut reflected: Vec<_> = face.corners.iter().map(|&i| mirror_ids[i as usize]).collect();
                reflected.sort_unstable();
                assert!(face_keys.contains(&reflected));
            }
            assert_subdivides(&cage);
        }
    }

    #[test]
    fn reflected_limb_triangles_use_the_same_diagonals() {
        for level in [0, 1, 2] {
            let mesh =
                quad_skin_mesh(&fork(0.8, 3.0), QuadSkinOptions { subdivision: level, ..Default::default() }).unwrap();
            let triangles: BTreeSet<_> = mesh
                .triangles
                .iter()
                .map(|t| {
                    let mut key = *t;
                    key.sort_unstable();
                    key
                })
                .collect();
            let mut checked = 0;
            for t in &mesh.triangles {
                if t.iter().all(|&i| mesh.positions[i as usize].x > 1e-5) {
                    let mut reflected = t.map(|i| {
                        let p = mesh.positions[i as usize];
                        let reflected = Vec3::new(-p.x, p.y, p.z);
                        mesh.positions.iter().position(|q| q.distance(reflected) < 2e-5).unwrap() as u32
                    });
                    reflected.sort_unstable();
                    assert!(triangles.contains(&reflected), "missing reflected triangle at subdivision {level}");
                    checked += 1;
                }
            }
            assert!(checked > 0);
        }
    }

    #[test]
    fn asymmetric_fork_with_a_bent_side_branch_subdivides() {
        let mut arm = Armature::new(Vec3::new(0.1, -3.0, 0.4), 0.44, None);
        let joint = arm.add_child(0, Vec3::ZERO, 0.33);
        let side = arm.add_child(joint, Vec3::new(2.1, 1.0, 0.8), 0.19);
        arm.add_child(side, Vec3::new(3.0, 2.3, 1.9), 0.08);
        arm.add_child(joint, Vec3::new(-1.4, 2.1, -0.45), 0.1);
        let cage = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
        assert_eq!(cage, quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap());
        assert!(cage.quad_count() > cage.triangle_count());
        assert_subdivides(&cage);
    }

    #[test]
    fn blend_and_resolution_change_the_cage() {
        let arm = fork(2.5, 1.0);
        let base = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
        let sharp = quad_skin_armature(&arm, QuadSkinOptions { blend: 0.0, ..Default::default() }).unwrap();
        let soft = quad_skin_armature(&arm, QuadSkinOptions { blend: 1.0, ..Default::default() }).unwrap();
        assert_ne!(sharp.positions, soft.positions);
        assert_eq!(base.faces.len(), sharp.faces.len());
        let dense = quad_skin_armature(&arm, QuadSkinOptions { resolution: 320, ..Default::default() }).unwrap();
        assert!(dense.quad_count() > base.quad_count(), "higher Resolution adds limb rings");
        assert_subdivides(&dense);
        assert_subdivides(&soft);
    }

    fn two_forks(distance: f32, bent: bool) -> Armature {
        let mut arm = Armature::new(Vec3::ZERO, 0.4, Some(0.0));
        arm.add_child(0, Vec3::new(2.0, -0.8, 0.0), 0.18);
        let parent = if bent { arm.add_child(0, Vec3::new(0.0, distance * 0.5, 0.35), 0.36) } else { 0 };
        let upper = arm.add_child(parent, Vec3::Y * distance, 0.4);
        arm.add_child(upper, Vec3::new(2.0, distance + 0.8, 0.0), 0.18);
        arm
    }

    #[test]
    fn two_junctions_share_one_quad_strip() {
        let opts = QuadSkinOptions::default();
        for bent in [false, true] {
            let arm = two_forks(3.0, bent);
            let cage = quad_skin_armature(&arm, opts).unwrap();
            assert_eq!(cage, quad_skin_armature(&arm, opts).unwrap());
            assert_eq!(cage.quad_count(), 64 + usize::from(bent) * SIDES);
            assert_subdivides(&cage);
        }
    }

    #[test]
    fn resolution_adds_rings_between_junction_collars() {
        let arm = two_forks(3.0, false);
        let base = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
        let dense = quad_skin_armature(&arm, QuadSkinOptions { resolution: 320, ..Default::default() }).unwrap();
        assert!(!base.positions.iter().any(|p| (1.0..2.0).contains(&p.y)));
        assert!(dense.positions.iter().any(|p| (1.0..2.0).contains(&p.y)));
        assert_subdivides(&dense);
    }

    #[test]
    fn overlapping_junction_collars_are_rejected() {
        assert_eq!(
            quad_skin_armature(&two_forks(0.9, false), QuadSkinOptions::default()),
            Err(QuadSkinError::CrowdedLinks)
        );
    }

    #[test]
    fn crowded_and_unsupported_inputs_fail() {
        let mut arm = fork(0.1, 0.4);
        assert_eq!(quad_skin_armature(&arm, QuadSkinOptions::default()), Err(QuadSkinError::CrowdedLinks));
        arm.add_child(1, Vec3::Z * 2.0, 0.1);
        assert_eq!(quad_skin_armature(&arm, QuadSkinOptions::default()), Err(QuadSkinError::UnsupportedBranching));

        let mut arm = Armature::new(Vec3::ZERO, 0.3, None);
        arm.add_child(0, Vec3::ZERO, 0.3);
        assert_eq!(quad_skin_armature(&arm, QuadSkinOptions::default()), Err(QuadSkinError::InvalidArmature));
    }

    #[test]
    fn long_vertical_chain_keeps_quad_strips() {
        let links = 256;
        let mut arm = Armature::new(Vec3::ZERO, 0.2, None);
        for i in 1..=links {
            arm.add_child(i - 1, Vec3::Y * i as f32, 0.2);
        }
        let cage = quad_skin_armature(&arm, QuadSkinOptions::default()).unwrap();
        assert_eq!(cage.quad_count(), (links + 1) * SIDES);
        assert_eq!(cage.triangle_count(), 0);
    }

    #[test]
    fn crossing_links_fail_even_when_each_link_is_long() {
        let mut arm = Armature::new(Vec3::new(-2.0, -2.0, 0.0), 0.15, None);
        let first = arm.add_child(0, Vec3::new(2.0, 2.0, 0.0), 0.15);
        let second = arm.add_child(first, Vec3::new(-2.0, 2.0, 0.0), 0.15);
        arm.add_child(second, Vec3::new(2.0, -2.0, 0.0), 0.15);
        for _ in 0..3 {
            assert_eq!(quad_skin_armature(&arm, QuadSkinOptions::default()), Err(QuadSkinError::InvalidSurface));
            for sphere in &mut arm.spheres {
                let p = sphere.center;
                sphere.center = Vec3::new(p.z, p.x, p.y);
            }
        }
    }
}
