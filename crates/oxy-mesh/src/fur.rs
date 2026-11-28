//! Surface-bound short fur. Roots stay attached to compatible triangles
//! as their vertices move. The renderer can draw one small strand mesh
//! with these transforms without adding pieces to the scene.

use crate::instances::{self, Orient, ScatterOpts};
use crate::{Mesh, MeshData};
use glam::{IVec3, Mat4, Vec2, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

pub const MAX_ROOTS: usize = instances::MAX_SCATTER_ROOTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Region {
    #[default]
    Whole,
    Masked,
    Group(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub region: Region,
    pub spacing: f32,
    pub length: f32,
    pub width: f32,
    pub seed: u32,
}

impl Settings {
    pub fn for_span(span: f32) -> Self {
        let span = span.max(0.001);
        Self { region: Region::Whole, spacing: span * 0.025, length: span * 0.045, width: span * 0.0025, seed: 1 }
    }

    pub fn valid(self) -> bool {
        [self.spacing, self.length, self.width].into_iter().all(|value| value.is_finite() && value > 0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Root {
    pub triangle: u32,
    pub weights: [f32; 3],
    pub key: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Coat {
    pub settings: Settings,
    pub topology: u64,
    pub roots: Arc<[Root]>,
    pub groom: Option<Groom>,
}

/// Three weighted guides for a strand. The binding is fixed while the host
/// deforms and while individual guide directions change.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Influence {
    pub guides: [u16; 3],
    pub weights: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Groom {
    pub guide_roots: Arc<[u32]>,
    pub influences: Arc<[Influence]>,
    /// Tangent-space heading. Magnitude is the amount the fur lies down.
    pub directions: Vec<Vec2>,
}

impl Groom {
    pub fn is_combed(&self) -> bool {
        self.directions.iter().any(|d| *d != Vec2::ZERO)
    }
}

#[derive(Clone, Copy)]
struct Frame {
    point: Vec3,
    normal: Vec3,
    tangent: Vec3,
}

impl Frame {
    fn bitangent(self) -> Vec3 {
        self.normal.cross(self.tangent)
    }

    fn world(self, direction: Vec2) -> Vec3 {
        self.tangent * direction.x + self.bitangent() * direction.y
    }
}

fn frame(root: &Root, positions: &[Vec3], normals: &[Vec3], triangles: &[[u32; 3]]) -> Option<Frame> {
    let tri = *triangles.get(root.triangle as usize)?;
    let point = interpolate(tri, root.weights, positions);
    let normal = interpolate(tri, root.weights, normals).try_normalize()?;
    let edge = positions[tri[1] as usize] - positions[tri[0] as usize];
    let tangent = (edge - normal * normal.dot(edge)).try_normalize().unwrap_or_else(|| normal.any_orthonormal_vector());
    Some(Frame { point, normal, tangent })
}

fn interpolate([a, b, c]: [u32; 3], [u, v, w]: [f32; 3], vertices: &[Vec3]) -> Vec3 {
    vertices[a as usize] * u + vertices[b as usize] * v + vertices[c as usize] * w
}

/// A saved signature of ordered triangles and point count. Geometry, masks,
/// and face visibility can change without invalidating the root coordinates.
pub fn topology_signature(points: usize, triangles: &[[u32; 3]]) -> u64 {
    let mut signature = 0xcbf2_9ce4_8422_2325u64;
    for value in [points as u32, triangles.len() as u32].into_iter().chain(triangles.iter().flatten().copied()) {
        signature = (signature ^ u64::from(value)).wrapping_mul(0x100_0000_01b3);
    }
    signature
}

impl Coat {
    pub fn generate(mesh: &MeshData, settings: Settings) -> Self {
        let mask = (settings.region == Region::Masked).then_some((mesh.mask.as_slice(), 0.5));
        let group = match settings.region {
            Region::Group(group) => Some((mesh.groups.as_slice(), group)),
            _ => None,
        };
        let roots = if settings.valid() {
            instances::scatter_surface(
                &mesh.positions,
                &mesh.triangles,
                &mesh.hidden,
                ScatterOpts { spacing: settings.spacing, seed: settings.seed, mask, group, orient: Orient::World },
                |triangle, anchor| Root {
                    triangle,
                    weights: [anchor.weights[0], anchor.weights[1], anchor.weights[2]],
                    key: anchor.key,
                },
            )
        } else {
            Vec::new()
        };
        Self {
            settings,
            topology: topology_signature(mesh.positions.len(), &mesh.triangles),
            roots: roots.into(),
            groom: None,
        }
    }

    pub fn fits(&self, mesh: &Mesh) -> bool {
        self.topology == topology_signature(mesh.vertex_count(), &mesh.triangles)
    }

    /// Rebind saved face indices after Mesh::from_data packs triangles
    /// into chunks. It keeps the corner order within each triangle.
    pub fn renumber_after_load(&mut self, mesh: &Mesh) -> bool {
        let mut faces: HashMap<_, _> = self.roots.iter().map(|root| (root.triangle, None)).collect();
        for (index, old) in mesh.source_triangles().iter().enumerate() {
            if let Some(current) = faces.get_mut(old) {
                *current = Some(index as u32);
            }
        }
        if faces.values().any(Option::is_none) {
            return false;
        }
        for root in Arc::make_mut(&mut self.roots) {
            root.triangle = faces[&root.triangle].expect("validated root face");
        }
        self.topology = topology_signature(mesh.vertex_count(), &mesh.triangles);
        true
    }

    /// Bind local guides in the pose in which the groom was first started.
    /// Choose one guide per cell and surface side in deterministic root order.
    /// Each strand blends up to three same-side guides in nearby cells.
    pub fn bind_groom(&self, positions: &[Vec3], normals: &[Vec3], triangles: &[[u32; 3]]) -> Option<Groom> {
        let poses: Vec<_> = self.roots.iter().map(|root| frame(root, positions, normals, triangles)).collect();
        let origin = poses.iter().flatten().next()?.point;
        let cell_size = self.settings.spacing * 3.0;
        let cell =
            |point: Vec3| ((point - origin) / cell_size).clamp(Vec3::splat(-1e8), Vec3::splat(1e8)).floor().as_ivec3();
        let mut cells: HashMap<IVec3, Vec<u16>> = HashMap::new();
        let mut guide_roots: Vec<u32> = Vec::new();
        for (index, pose) in poses.iter().enumerate() {
            let Some(pose) = pose else { continue };
            let guides = cells.entry(cell(pose.point)).or_default();
            if guides.iter().any(|&g| {
                poses[guide_roots[g as usize] as usize].is_some_and(|other| other.normal.dot(pose.normal) > 0.5)
            }) {
                continue;
            }
            guides.push(guide_roots.len() as u16);
            guide_roots.push(index as u32);
        }
        let mut influences = vec![Influence::default(); poses.len()];
        let minimum = (self.settings.spacing * 0.1).powi(2).max(f32::MIN_POSITIVE);
        for (index, pose) in poses.iter().enumerate() {
            let Some(pose) = pose else { continue };
            let center = cell(pose.point);
            let mut best = [(f32::INFINITY, 0u16); 3];
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        for &guide in cells.get(&(center + IVec3::new(x, y, z))).into_iter().flatten() {
                            let other = poses[guide_roots[guide as usize] as usize].expect("guide has a pose");
                            if other.normal.dot(pose.normal) <= 0.5 {
                                continue;
                            }
                            let dist = other.point.distance_squared(pose.point);
                            let at = best.partition_point(|&(d, id)| (d, id) < (dist, guide));
                            if at < best.len() {
                                best[at..].rotate_right(1);
                                best[at] = (dist, guide);
                            }
                        }
                    }
                }
            }
            let weights = best.map(|(dist, _)| minimum / dist.max(minimum));
            let total: f32 = weights.iter().sum();
            if total > 0.0 {
                influences[index] =
                    Influence { guides: best.map(|(_, guide)| guide), weights: weights.map(|w| w / total) };
            }
        }
        let directions = vec![Vec2::ZERO; guide_roots.len()];
        Some(Groom { guide_roots: guide_roots.into(), influences: influences.into(), directions })
    }

    /// Comb the visible side of the surface along a swept brush segment.
    pub fn comb(&mut self, mesh: &Mesh, from: Vec3, to: Vec3, normal: Vec3, radius: f32, strength: f32) -> bool {
        let Some(groom) = &mut self.groom else { return false };
        let motion = to - from;
        let distance = motion.length();
        if distance < 1e-6 || !radius.is_finite() || radius <= 0.0 || strength <= 0.0 {
            return false;
        }
        let heading = motion / distance;
        let amount = (distance / radius * 4.0).min(1.0) * strength.clamp(0.0, 1.0);
        let mut changed = false;
        for (&root_index, direction) in groom.guide_roots.iter().zip(&mut groom.directions) {
            let root = &self.roots[root_index as usize];
            if mesh.hidden()[root.triangle as usize] {
                continue;
            }
            let Some(pose) = frame(root, &mesh.positions, &mesh.normals, &mesh.triangles) else { continue };
            if pose.normal.dot(normal) <= 0.25 {
                continue;
            }
            let closest = from + heading * (pose.point - from).dot(heading).clamp(0.0, distance);
            let u = pose.point.distance(closest) / radius;
            if u >= 1.0 {
                continue;
            }
            let projected = Vec2::new(heading.dot(pose.tangent), heading.dot(pose.bitangent()));
            let Some(target) = projected.try_normalize() else { continue };
            let falloff = (1.0 - u).powi(2) * (1.0 + 2.0 * u);
            let next = direction.lerp(target, falloff * amount);
            if next.distance_squared(*direction) > 1e-8 {
                *direction = next;
                changed = true;
            }
        }
        changed
    }

    /// Called after checking the topology signature. Hidden faces are filtered
    /// at draw time so toggling visibility keeps the same roots.
    pub fn transforms(&self, mesh: &Mesh) -> Vec<Mat4> {
        let settings = self.settings;
        let groom = self.groom.as_ref().filter(|g| g.is_combed());
        let guide_world: Vec<Vec3> = groom.map_or_else(Vec::new, |groom| {
            groom
                .guide_roots
                .iter()
                .zip(&groom.directions)
                .map(|(&root, &dir)| {
                    if dir == Vec2::ZERO {
                        return Vec3::ZERO;
                    }
                    frame(&self.roots[root as usize], &mesh.positions, &mesh.normals, &mesh.triangles)
                        .map_or(Vec3::ZERO, |pose| pose.world(dir))
                })
                .collect()
        });
        self.roots
            .iter()
            .enumerate()
            .filter_map(|(index, root)| {
                if mesh.hidden().get(root.triangle as usize).copied()? {
                    return None;
                }
                let tri = mesh.triangles[root.triangle as usize];
                let mut up = interpolate(tri, root.weights, &mesh.normals).try_normalize()?;
                let spin = std::f32::consts::TAU * instances::random(settings.seed, root.key, 8);
                let (sin, cos) = spin.sin_cos();
                let tangent = up.any_orthonormal_vector();
                let bitangent = up.cross(tangent);
                let mut forward = tangent * cos + bitangent * sin;
                if let Some(groom) = groom {
                    let influence = &groom.influences[index];
                    let mut flow: Vec3 = influence
                        .guides
                        .iter()
                        .zip(influence.weights)
                        .map(|(&guide, weight)| guide_world[guide as usize] * weight)
                        .sum();
                    flow -= up * flow.dot(up);
                    if let Some(heading) = flow.try_normalize() {
                        let lean = flow.length().min(1.0);
                        forward = forward.lerp(heading, lean).normalize_or(heading);
                        up = up.lerp(heading, 0.64 * lean).normalize();
                        forward = (forward - up * forward.dot(up)).normalize_or(bitangent);
                    }
                }
                let right = up.cross(forward);
                let length = settings.length * (0.75 + 0.5 * instances::random(settings.seed, root.key, 9));
                Some(Mat4::from_cols(
                    (right * settings.width).extend(0.0),
                    (up * length).extend(0.0),
                    (forward * length).extend(0.0),
                    interpolate(tri, root.weights, &mesh.positions).extend(1.0),
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comb_stroke_changes_local_guides_and_keeps_root_positions() {
        let mesh = crate::primitives::plane(8, 2.0);
        let mut coat = Coat::generate(&mesh.to_data(), Settings::for_span(2.0));
        let bare = coat.transforms(&mesh);
        let groom = coat.bind_groom(&mesh.positions, &mesh.normals, &mesh.triangles).unwrap();
        assert!(groom.guide_roots.len() < coat.roots.len());
        assert_eq!(groom, coat.bind_groom(&mesh.positions, &mesh.normals, &mesh.triangles).unwrap());
        coat.groom = Some(groom);
        assert_eq!(coat.transforms(&mesh), bare);
        assert!(coat.comb(&mesh, -Vec3::X * 0.35, Vec3::X * 0.35, Vec3::Y, 0.35, 1.0));
        let groomed = coat.transforms(&mesh);
        assert!(groomed.iter().any(|m| m.y_axis.x > 0.02));
        for (old, new) in bare.into_iter().zip(groomed) {
            assert_eq!(old.w_axis, new.w_axis);
            if old.w_axis.z.abs() > 0.95 {
                assert_eq!(old, new);
            }
        }
        let before = coat.groom.clone();
        assert!(!coat.comb(&mesh, -Vec3::X * 0.35, Vec3::X * 0.35, -Vec3::Y, 0.35, 1.0));
        assert_eq!(coat.groom, before);
    }

    #[test]
    fn seeded_roots_follow_geometry_and_respect_regions_and_visibility() {
        let mut mesh = crate::primitives::plane(8, 2.0);
        let mut data = mesh.to_data();
        data.groups.iter_mut().zip(&data.triangles).for_each(|(group, tri)| {
            *group = u32::from(tri.iter().all(|&i| data.positions[i as usize].x >= 0.0));
        });
        data.mask.iter_mut().zip(&data.positions).for_each(|(mask, p)| *mask = f32::from(u8::from(p.x >= 0.0)));
        let settings = Settings { region: Region::Group(1), ..Settings::for_span(2.0) };
        let coat = Coat::generate(&data, settings);
        assert_eq!(coat, Coat::generate(&data, settings));
        assert!(!coat.roots.is_empty());
        assert!(coat.roots.iter().all(|root| data.groups[root.triangle as usize] == 1));
        let masked = Coat::generate(&data, Settings { region: Region::Masked, ..settings });
        assert!(masked.roots.iter().all(|root| {
            data.triangles[root.triangle as usize].iter().filter(|&&i| data.mask[i as usize] == 1.0).count() >= 2
        }));
        assert!(coat.fits(&mesh));
        let original = coat.transforms(&mesh);
        let delta = Vec3::new(0.3, 0.7, -0.1);
        mesh.replace_positions(mesh.positions.iter().map(|p| *p + delta).collect());
        assert!(coat.fits(&mesh));
        for (before, after) in original.iter().zip(coat.transforms(&mesh)) {
            assert!((after.w_axis.truncate() - before.w_axis.truncate() - delta).length() < 1e-5);
        }
        mesh.set_hidden(vec![true; mesh.triangle_count()]);
        assert!(coat.transforms(&mesh).is_empty());
        assert!(!coat.fits(&crate::primitives::plane(6, 2.0)));
    }
}
