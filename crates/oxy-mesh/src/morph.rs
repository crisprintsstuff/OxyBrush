//! Position blending for two variants of the same indexed mesh.

use crate::{Mesh, Vec3};
use rayon::prelude::*;

/// Blend positions in the shared vertex order. A fork made by cloning a
/// mesh retains its topology version through position edits. A rebuilt or
/// independently loaded mesh needs its own correspondence before blending.
pub fn blend_positions(a: &Mesh, b: &Mesh, mix: f32) -> Option<Vec<Vec3>> {
    if !mix.is_finite() || a.topology_version() != b.topology_version() || a.vertex_count() != b.vertex_count() {
        return None;
    }
    if mix <= 0.0 {
        return Some(a.positions.clone());
    }
    if mix >= 1.0 {
        return Some(b.positions.clone());
    }
    Some(a.positions.par_iter().zip(&b.positions).map(|(&a, &b)| a.lerp(b, mix)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn blend_uses_the_same_vertices_and_preserves_exact_endpoints() {
        let a = primitives::cube(2, 1.0);
        let mut b = a.clone();
        b.replace_positions(a.positions.iter().map(|p| *p + Vec3::splat(2.0)).collect());
        assert_eq!(blend_positions(&a, &b, 0.0).unwrap(), a.positions);
        assert_eq!(blend_positions(&a, &b, 1.0).unwrap(), b.positions);
        let expected: Vec<_> = a.positions.iter().map(|p| *p + Vec3::splat(0.5)).collect();
        assert_eq!(blend_positions(&a, &b, 0.25).unwrap(), expected);
        assert!(blend_positions(&a, &b, f32::NAN).is_none());
        assert!(blend_positions(&a, &primitives::cube(2, 1.0), 0.5).is_none());
    }
}
