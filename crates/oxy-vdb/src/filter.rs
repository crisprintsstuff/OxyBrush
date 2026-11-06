//! Volumetric level set filtering, smoothing, and morphological operations.
//!
//! Mirrors `openvdb::tools::LevelSetFilter`.

use crate::tree::{LEAF_VOXELS, VdbFloatGrid};
use glam::IVec3;
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    Laplacian { factor: f32 },
    Gaussian,
    Mean,
}

/// Applies an offset to the level set surface.
///
/// Positive offset moves the surface outward (dilation), negative moves it inward (erosion).
pub fn offset(grid: &mut VdbFloatGrid, distance: f32) {
    let leaves: Vec<_> = grid.leaves.values_mut().collect();
    leaves.into_par_iter().for_each(|leaf| {
        for offset in 0..LEAF_VOXELS {
            if leaf.mask.is_set(offset) {
                leaf.values[offset] -= distance;
            }
        }
    });
}

/// Converts a solid volume into a hollow shell of uniform wall thickness.
pub fn shell(grid: &mut VdbFloatGrid, wall_thickness: f32) {
    let half_thick = (wall_thickness * 0.5).max(1e-5);
    let leaves: Vec<_> = grid.leaves.values_mut().collect();
    leaves.into_par_iter().for_each(|leaf| {
        for offset in 0..LEAF_VOXELS {
            if leaf.mask.is_set(offset) {
                let d = leaf.values[offset];
                leaf.values[offset] = d.abs() - half_thick;
            }
        }
    });
}

/// Smooths the level set using 3D Laplacian relaxation on the active voxels.
pub fn smooth_laplacian(grid: &mut VdbFloatGrid, passes: u32, factor: f32) {
    let factor = factor.clamp(0.01, 0.15); // Stable timestep for 3D diffusion

    const NEIGHBORS: [IVec3; 6] = [
        IVec3::new(1, 0, 0),
        IVec3::new(-1, 0, 0),
        IVec3::new(0, 1, 0),
        IVec3::new(0, -1, 0),
        IVec3::new(0, 0, 1),
        IVec3::new(0, 0, -1),
    ];

    for _ in 0..passes {
        let snapshot = grid.clone();
        let leaves: Vec<_> = grid.leaves.values_mut().collect();

        leaves.into_par_iter().for_each(|leaf| {
            for offset in 0..LEAF_VOXELS {
                if leaf.mask.is_set(offset) {
                    let coord = leaf.coord_of(offset);
                    let val = snapshot.get_value(coord);

                    let mut sum = 0.0;
                    for &n in &NEIGHBORS {
                        sum += snapshot.get_value(coord + n);
                    }
                    let laplacian = sum - 6.0 * val;
                    leaf.values[offset] = val + factor * laplacian;
                }
            }
        });
    }
}
