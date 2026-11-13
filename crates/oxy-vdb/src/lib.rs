//! Hierarchical sparse VDB grid, voxel remeshing, volumetric CSG booleans,
//! and level set filters for digital sculpting.
//!
//! Clean-room implementation in pure, safe Rust based on published academic literature:
//! - Sparse tree architecture: Museth (2013), "VDB: High-Resolution Sparse Volumes with Dynamic Topology", ACM Trans. Graph. 32(3).
//! - Signed distance determination: Bærentzen & Aanæs (2005), "Signed Distance Computation Using the Angle Weighted Pseudonormal", IEEE TVCG 11(3); Ericson (2005), "Real-Time Collision Detection" §5.1.5.
//! - Dual contouring polygonization: Ju, Losasso, Schaefer, Warren (2002), "Dual Contouring of Hermite Data", ACM SIGGRAPH 2002.
//! - Volumetric CSG operations: Exact R-functions on signed distance fields (Pasko et al., FRep).
//!
//! All algorithms and data structures were written from scratch without consulting or adapting any C++ source code.

pub mod csg;
pub mod filter;
pub mod mesh_to_volume;
pub mod tree;
pub mod volume_to_mesh;

pub use csg::{BooleanOp, csg_difference, csg_intersection, csg_operation, csg_union};
pub use filter::{FilterType, offset, shell, smooth_laplacian};
pub use mesh_to_volume::{
    MeshToVolumeOptions, SignOverride, Signing, closest_point_on_triangle, mesh_to_level_set, mesh_to_level_set_signed,
};
pub use tree::{BitMask512, LEAF_DIM, LEAF_VOXELS, LeafNode, Transform, VdbFloatGrid};
pub use volume_to_mesh::{VolumeToMeshOptions, volume_to_mesh};

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use oxy_mesh::primitives;

    /// Every active voxel of the narrow band against a brute-force distance
    /// to all triangles: exact within a voxel of the surface, within a small
    /// fraction of a voxel further out, and with the right sign.
    fn check_band(mesh: &oxy_mesh::Mesh, voxel_size: f32, inside: impl Fn(Vec3) -> bool) {
        let grid =
            mesh_to_level_set(&mesh.positions, &mesh.triangles, MeshToVolumeOptions { voxel_size, half_width: 3.5 });
        let tris: Vec<[Vec3; 3]> = mesh.triangles.iter().map(|t| t.map(|i| mesh.positions[i as usize])).collect();
        let band = 3.5 * voxel_size;
        let (mut worst_near, mut worst_far, mut checked) = (0.0f32, 0.0f32, 0);
        for leaf in grid.leaves.values() {
            for off in 0..tree::LEAF_VOXELS {
                if !leaf.mask.is_set(off) {
                    continue;
                }
                let v = leaf.values[off];
                if v.abs() >= band {
                    continue;
                }
                let p = grid.transform.index_to_world(leaf.coord_of(off));
                let exact = tris
                    .iter()
                    .map(|t| p.distance(mesh_to_volume::closest_point_on_triangle(p, t[0], t[1], t[2])))
                    .fold(f32::MAX, f32::min);
                let err = (v.abs() - exact).abs() / voxel_size;
                if exact <= voxel_size {
                    worst_near = worst_near.max(err);
                } else {
                    worst_far = worst_far.max(err);
                }
                if exact > 0.5 * voxel_size {
                    assert_eq!(v < 0.0, inside(p), "sign at {p} (distance {exact})");
                }
                checked += 1;
            }
        }
        assert!(checked > 1000, "{checked}");
        assert!(worst_near < 1e-4, "near the surface the distance is exact: off by {worst_near} voxels");
        assert!(worst_far < 0.1, "further out it is close: off by {worst_far} voxels");
    }

    #[test]
    fn voxels_beyond_the_band_keep_their_side() {
        // Leaves reach past the narrow band; their voxels deep inside must
        // read as inside (CSG and the 3D-print tools rely on it).
        let sphere = primitives::icosphere(4, 1.0);
        let h = 2.0 / 40.0;
        let grid = mesh_to_level_set(
            &sphere.positions,
            &sphere.triangles,
            MeshToVolumeOptions { voxel_size: h, half_width: 3.5 },
        );
        let mut deep = 0;
        for leaf in grid.leaves.values() {
            for off in 0..tree::LEAF_VOXELS {
                let p = grid.transform.index_to_world(leaf.coord_of(off));
                let r = p.length();
                if r < 1.0 - 4.0 * h {
                    assert!(leaf.values[off] < 0.0 && leaf.mask.is_set(off), "inside at {p}");
                    deep += 1;
                } else if r > 1.0 + 4.0 * h {
                    assert!(leaf.values[off] > 0.0, "outside at {p}");
                }
            }
        }
        assert!(deep > 0);
    }

    #[test]
    fn narrow_band_distances_match_brute_force() {
        // A dense sphere: triangles much smaller than voxels.
        let sphere = primitives::icosphere(4, 1.0);
        check_band(&sphere, 2.0 / 40.0, |p| p.length() < 1.0);
        // The same sphere with voxels smaller than its triangles.
        check_band(&primitives::icosphere(2, 1.0), 2.0 / 64.0, |p| p.length() < 0.99);
        // A torus: concave, with a hole.
        let torus = primitives::torus(48, 24, 1.0, 0.35);
        check_band(&torus, 2.7 / 64.0, |p| {
            let ring = Vec3::new(p.x, 0.0, p.z).normalize_or_zero();
            p.distance(ring) < 0.35
        });
    }

    #[test]
    fn test_sphere_vdb_roundtrip() {
        let sphere = primitives::icosphere(3, 1.0);
        let grid = mesh_to_level_set(
            &sphere.positions,
            &sphere.triangles,
            MeshToVolumeOptions { voxel_size: 0.08, half_width: 3.0 },
        );

        assert!(grid.leaf_count() > 0, "should have active leaf nodes");
        assert!(grid.active_voxel_count() > 100, "should have active voxels");

        // Voxels slightly inside the sphere should have negative SDF (interior)
        let inside_val = grid.eval_trilinear(Vec3::new(0.95, 0.0, 0.0));
        assert!(inside_val < 0.0, "interior of sphere must be negative, got {inside_val}");
        let outside_val = grid.eval_trilinear(Vec3::new(1.05, 0.0, 0.0));
        assert!(outside_val > 0.0, "exterior of sphere must be positive, got {outside_val}");

        // Extract mesh back
        let remeshed = volume_to_mesh(&grid, VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 })
            .expect("should extract mesh from sphere grid");

        assert!(remeshed.vertex_count() > 100);
        assert!(remeshed.triangle_count() > 100);

        // Check radial distance of extracted vertices
        for p in &remeshed.positions {
            assert!((p.length() - 1.0).abs() < 0.12, "vertex should be close to radius 1.0, got length {}", p.length());
        }
    }

    #[test]
    fn test_vdb_csg_difference() {
        let sphere_a = primitives::icosphere(3, 1.0);
        let mut grid_a = mesh_to_level_set(
            &sphere_a.positions,
            &sphere_a.triangles,
            MeshToVolumeOptions { voxel_size: 0.08, half_width: 3.0 },
        );

        // Create a cutter sphere shifted slightly
        let sphere_b = primitives::icosphere(3, 0.7);
        let cutter_positions: Vec<Vec3> = sphere_b.positions.iter().map(|&p| p + Vec3::new(0.5, 0.0, 0.0)).collect();
        let grid_b = mesh_to_level_set(
            &cutter_positions,
            &sphere_b.triangles,
            MeshToVolumeOptions { voxel_size: 0.08, half_width: 3.0 },
        );

        // Subtract grid_b from grid_a
        csg_difference(&mut grid_a, &grid_b);

        let remeshed =
            volume_to_mesh(&grid_a, VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 0 })
                .expect("should produce mesh from CSG difference");

        // The point (0.5, 0.0, 0.0) where the cutter was centered should now be hollowed out / not inside!
        for p in &remeshed.positions {
            // No vertices should be inside cutter's deep interior
            assert!(
                p.distance(Vec3::new(0.5, 0.0, 0.0)) > 0.4 || p.length() > 0.8,
                "vertex {} improperly inside difference void",
                p
            );
        }
    }

    #[test]
    fn test_vdb_csg_union() {
        let sphere_a = primitives::icosphere(3, 1.0);
        let mut grid_a = mesh_to_level_set(
            &sphere_a.positions,
            &sphere_a.triangles,
            MeshToVolumeOptions { voxel_size: 0.08, half_width: 3.0 },
        );

        let sphere_b = primitives::icosphere(3, 1.0);
        let shifted_positions: Vec<Vec3> = sphere_b.positions.iter().map(|&p| p + Vec3::new(1.0, 0.0, 0.0)).collect();
        let grid_b = mesh_to_level_set(
            &shifted_positions,
            &sphere_b.triangles,
            MeshToVolumeOptions { voxel_size: 0.08, half_width: 3.0 },
        );

        csg_union(&mut grid_a, &grid_b);

        let remeshed =
            volume_to_mesh(&grid_a, VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 })
                .expect("should produce mesh from CSG union");

        assert!(remeshed.bounds().extent().x > 2.5, "union should span across both spheres");
    }

    #[test]
    fn test_vdb_offset() {
        let sphere = primitives::icosphere(3, 1.0);
        let mut grid = mesh_to_level_set(
            &sphere.positions,
            &sphere.triangles,
            MeshToVolumeOptions { voxel_size: 0.08, half_width: 5.0 },
        );

        // Dilate outward by 0.15
        offset(&mut grid, 0.15);

        let remeshed = volume_to_mesh(&grid, VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 0 })
            .expect("should extract dilated mesh");

        for p in &remeshed.positions {
            assert!((p.length() - 1.15).abs() < 0.15, "dilated sphere radius should be ~1.15, got {}", p.length());
        }
    }
}
