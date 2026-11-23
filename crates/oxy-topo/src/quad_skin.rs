//! Headless polygon skin for OxySpheres (RFC 0038).

use glam::{Quat, Vec3};
use oxy_mesh::armature::{Armature, Sphere};
use oxy_mesh::retopo::{Cage, Face, edge_key};
use std::collections::{BTreeSet, HashMap, HashSet};

const SIDES: usize = 8;
type Ring = [u32; SIDES];

#[derive(Clone, Copy)]
struct Section {
    ring: Ring,
    center: Vec3,
    normal: Vec3,
    axis: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadSkinError {
    InvalidArmature,
    UnsupportedBranching,
    CrowdedLinks,
    InvalidSurface,
}

impl std::fmt::Display for QuadSkinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidArmature => "The armature needs at least two valid, distinct joints",
            Self::UnsupportedBranching => "Quad skin supports up to three links at each joint",
            Self::CrowdedLinks => "The links are too short or close together for the quad skin",
            Self::InvalidSurface => "The quad skin would overlap or produce invalid faces",
        })
    }
}

impl std::error::Error for QuadSkinError {}

/// Build a closed polygon cage around a tree of chains and three-way junctions. The armature stays unchanged. Use `Cage::build_mesh` to author a
/// sculpt mesh with the polygon boundaries intact.
pub fn quad_skin_armature(armature: &Armature) -> Result<Cage, QuadSkinError> {
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
        ports[joint] = junction_ports(&mut cage, spheres, spheres[joint], adjacent)?;
    }
    let tree = SweepTree { spheres, neighbors: &neighbors, ports: &ports };
    if ports.iter().any(|rings| !rings.is_empty()) {
        // Each path has at most two junction ends. Mark the far port so
        // a path connecting two patches is swept only once.
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

fn junction_ports(
    cage: &mut Cage,
    spheres: &[Sphere],
    origin: Sphere,
    branches: &[usize],
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
        let offset = origin.radius * (0.35 + 1.15 / (angle * 0.5).tan());
        if !offset.is_finite() || offset >= length * 0.65 {
            return Err(CrowdedLinks);
        }
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
    Section { ring, center, normal: direction, axis }
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
}

/// Sweep away from the start, returning the far port if it ends at a junction.
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
            // Two collars can each fit their own junction and still
            // consume more than the shared link.
            if (end.center - section.center).dot(incoming) <= 1e-6 {
                return Err(QuadSkinError::CrowdedLinks);
            }
            let transported = Quat::from_rotation_arc(section.normal, -end.normal) * section.axis;
            let offset = (0..SIDES)
                .max_by(|&a, &b| {
                    let alignment = |i| (cage.positions[end.ring[i] as usize] - end.center).dot(transported);
                    alignment(a).total_cmp(&alignment(b))
                })
                .expect("ring has vertices");
            // The far collar faces the swept path. Reverse it for a
            // consistently wound strip and phase it to minimize twist.
            let aligned = std::array::from_fn(|i| end.ring[(offset + SIDES - i) % SIDES]);
            bridge_rings(cage, section.ring, aligned);
            return Ok(Some((current, previous)));
        }

        let next = tree.neighbors[current].iter().copied().find(|&index| index != previous);
        let outgoing = next.map_or(incoming, |i| (tree.spheres[i].center - sphere.center).normalize());
        let direction = (incoming + outgoing).normalize_or_zero();
        if incoming.dot(outgoing) < -0.8 || section.normal.dot(direction) < -0.8 {
            return Err(QuadSkinError::CrowdedLinks);
        }
        let axis = Quat::from_rotation_arc(section.normal, direction) * section.axis;
        let end = add_ring(cage, sphere.center, sphere.radius, direction, axis);
        bridge_rings(cage, section.ring, end.ring);
        let Some(next) = next else {
            cap(cage, end.ring, sphere.center + direction * sphere.radius, false);
            return Ok(None);
        };
        (previous, current, section) = (current, next, end);
    }
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
    fn straight_and_bent_tapers_keep_quad_strips() {
        for bend in [0.0, 1.2] {
            let mut arm = Armature::new(Vec3::ZERO, 0.4, None);
            let middle = arm.add_child(0, Vec3::Y * 2.0, 0.3);
            arm.add_child(middle, Vec3::new(bend, 4.0, 0.0), 0.15);
            let cage = quad_skin_armature(&arm).unwrap();
            assert_eq!(cage.quad_count(), 3 * SIDES);
            assert_eq!(cage.triangle_count(), 0);
            assert_subdivides(&cage);
        }
    }

    #[test]
    fn acute_and_wide_forks_are_deterministic_and_mirrored() {
        for (width, height) in [(0.8, 3.0), (2.5, 1.0)] {
            let arm = fork(width, height);
            let cage = quad_skin_armature(&arm).unwrap();
            assert_eq!(cage, quad_skin_armature(&arm).unwrap());
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
    fn asymmetric_fork_with_a_bent_side_branch_subdivides() {
        let mut arm = Armature::new(Vec3::new(0.1, -3.0, 0.4), 0.44, None);
        let joint = arm.add_child(0, Vec3::ZERO, 0.33);
        let side = arm.add_child(joint, Vec3::new(2.1, 1.0, 0.8), 0.19);
        arm.add_child(side, Vec3::new(3.0, 2.3, 1.9), 0.08);
        arm.add_child(joint, Vec3::new(-1.4, 2.1, -0.45), 0.1);
        let cage = quad_skin_armature(&arm).unwrap();
        assert_eq!(cage, quad_skin_armature(&arm).unwrap());
        assert!(cage.quad_count() > cage.triangle_count());
        assert_subdivides(&cage);
    }

    fn two_forks(distance: f32, bent: bool) -> Armature {
        let mut arm = Armature::new(Vec3::ZERO, 0.4, None);
        arm.add_child(0, Vec3::new(-2.0, -0.8, 0.0), 0.18);
        arm.add_child(0, Vec3::new(2.0, -0.8, 0.0), 0.18);
        let parent = if bent { arm.add_child(0, Vec3::new(0.0, distance * 0.5, 0.35), 0.36) } else { 0 };
        let upper = arm.add_child(parent, Vec3::Y * distance, 0.4);
        arm.add_child(upper, Vec3::new(-2.0, distance + 0.8, 0.0), 0.18);
        arm.add_child(upper, Vec3::new(2.0, distance + 0.8, 0.0), 0.18);
        arm
    }

    #[test]
    fn two_junctions_share_one_quad_strip() {
        for bent in [false, true] {
            let arm = two_forks(3.0, bent);
            let cage = quad_skin_armature(&arm).unwrap();
            assert_eq!(cage, quad_skin_armature(&arm).unwrap());
            assert_eq!(cage.quad_count(), 64 + usize::from(bent) * SIDES);
            assert_subdivides(&cage);
        }
    }

    #[test]
    fn overlapping_junction_collars_are_rejected() {
        assert_eq!(quad_skin_armature(&two_forks(0.9, false)), Err(QuadSkinError::CrowdedLinks));
    }

    #[test]
    fn crowded_and_unsupported_inputs_fail() {
        let mut arm = fork(0.1, 0.4);
        assert_eq!(quad_skin_armature(&arm), Err(QuadSkinError::CrowdedLinks));
        arm.add_child(1, Vec3::Z * 2.0, 0.1);
        assert_eq!(quad_skin_armature(&arm), Err(QuadSkinError::UnsupportedBranching));

        let mut arm = Armature::new(Vec3::ZERO, 0.3, None);
        arm.add_child(0, Vec3::ZERO, 0.3);
        assert_eq!(quad_skin_armature(&arm), Err(QuadSkinError::InvalidArmature));
    }

    #[test]
    fn long_vertical_chain_keeps_quad_strips() {
        let links = 256;
        let mut arm = Armature::new(Vec3::ZERO, 0.2, None);
        for i in 1..=links {
            arm.add_child(i - 1, Vec3::Y * i as f32, 0.2);
        }
        let cage = quad_skin_armature(&arm).unwrap();
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
            assert_eq!(quad_skin_armature(&arm), Err(QuadSkinError::InvalidSurface));
            for sphere in &mut arm.spheres {
                let p = sphere.center;
                sphere.center = Vec3::new(p.z, p.x, p.y);
            }
        }
    }
}
