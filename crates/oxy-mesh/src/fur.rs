//! Surface-bound short fur. Roots stay attached to compatible triangles
//! as their vertices move. The renderer can draw one small strand mesh
//! with these transforms without adding pieces to the scene.

use crate::instances::{self, Orient, ScatterOpts};
use crate::{Mesh, MeshData};
use glam::Mat4;
use std::collections::HashMap;

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
    pub roots: Vec<Root>,
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
        Self { settings, topology: topology_signature(mesh.positions.len(), &mesh.triangles), roots }
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
        for root in &mut self.roots {
            root.triangle = faces[&root.triangle].expect("validated root face");
        }
        self.topology = topology_signature(mesh.vertex_count(), &mesh.triangles);
        true
    }

    /// Called after checking the topology signature. Hidden faces are filtered
    /// at draw time so toggling visibility keeps the same roots.
    pub fn transforms(&self, mesh: &Mesh) -> Vec<Mat4> {
        let settings = self.settings;
        self.roots
            .iter()
            .filter_map(|root| {
                let index = root.triangle as usize;
                let tri = *mesh.triangles.get(index)?;
                if mesh.hidden()[index] {
                    return None;
                }
                let [a, b, c] = tri.map(|vertex| mesh.positions[vertex as usize]);
                let [na, nb, nc] = tri.map(|vertex| mesh.normals[vertex as usize]);
                let [u, v, w] = root.weights;
                let pos = a * u + b * v + c * w;
                let up = (na * u + nb * v + nc * w).try_normalize()?;
                let spin = std::f32::consts::TAU * instances::random(settings.seed, root.key, 8);
                let (sin, cos) = spin.sin_cos();
                let tangent = up.any_orthonormal_vector();
                let bitangent = up.cross(tangent);
                let forward = tangent * cos + bitangent * sin;
                let right = up.cross(forward);
                let length = settings.length * (0.75 + 0.5 * instances::random(settings.seed, root.key, 9));
                Some(Mat4::from_cols(
                    (right * settings.width).extend(0.0),
                    (up * length).extend(0.0),
                    (forward * length).extend(0.0),
                    pos.extend(1.0),
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

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
