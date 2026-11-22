//! Constructive Solid Geometry (CSG) volumetric boolean operations on VDB level sets.
//!
//! Uses exact R-function algebra on signed distance fields (Pasko et al., Function Representation in Geometric Modeling):
//! - Union: target ∪ tool = min(d_A, d_B)
//! - Difference: target \ tool = max(d_A, -d_B)
//! - Intersection: target ∩ tool = max(d_A, d_B)
//!
//! With a blend radius `k` the hard `min`/`max` become the polynomial
//! smooth minimum and maximum (Quilez, "Smooth minimum", 2013):
//! `smin(a, b, k) = min(a, b) - k/4 * max(k - |a - b|, 0)^2 / k^2`. Where the
//! two surfaces cross, the hard combination makes a crease sharper than a
//! voxel, which surface extraction can only follow as a sawtooth; the
//! blend rounds it into a fillet about `k/4` deep that the grid resolves.
//! Away from the crease (|a - b| >= k) the result is exact.

use crate::tree::{LEAF_VOXELS, LeafNode, VdbFloatGrid};
use glam::IVec3;
use rayon::prelude::*;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}

/// In-place CSG Boolean operation modifying `target` with `tool`:
/// - `Union`: `target` ∪ `tool`
/// - `Difference`: `target` \ `tool`
/// - `Intersection`: `target` ∩ `tool`
pub fn csg_operation(target: &mut VdbFloatGrid, tool: &VdbFloatGrid, op: BooleanOp) {
    csg_operation_blended(target, tool, op, 0.0);
}

/// The polynomial smooth minimum of `a` and `b` over `k` (`min` when `k` is 0).
pub fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let t = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - t * t * k * 0.25
}

/// [`csg_operation`] with the crease where the two surfaces meet rounded
/// over `blend` (world units; 0: sharp). PRECONDITION: both grids' narrow
/// bands reach at least `blend` past the surface, or the fillet is cut
/// short where a value was clamped.
pub fn csg_operation_blended(target: &mut VdbFloatGrid, tool: &VdbFloatGrid, op: BooleanOp, blend: f32) {
    let bg_a = target.background;
    let bg_b = tool.background;
    let combined_bg = match op {
        BooleanOp::Union => bg_a.min(bg_b),
        BooleanOp::Difference => bg_a.max(-bg_b),
        BooleanOp::Intersection => bg_a.max(bg_b),
    };

    // Collect all leaf origins from both grids
    let all_origins: HashSet<IVec3> = target.leaves.keys().copied().chain(tool.leaves.keys().copied()).collect();

    let target_leaves = std::mem::take(&mut target.leaves);
    let tool_leaves = &tool.leaves;

    let origins_vec: Vec<IVec3> = all_origins.into_iter().collect();

    let updated_leaves: Vec<(IVec3, LeafNode)> = origins_vec
        .into_par_iter()
        .filter_map(|orig| {
            let leaf_a = target_leaves.get(&orig);
            let leaf_b = tool_leaves.get(&orig);

            let mut out_leaf = LeafNode::new(orig, combined_bg);
            let mut active_count = 0;

            for offset in 0..LEAF_VOXELS {
                let (va, active_a) =
                    if let Some(la) = leaf_a { (la.values[offset], la.mask.is_set(offset)) } else { (bg_a, false) };

                let (vb, active_b) =
                    if let Some(lb) = leaf_b { (lb.values[offset], lb.mask.is_set(offset)) } else { (bg_b, false) };

                let v_out = match op {
                    BooleanOp::Union => smooth_min(va, vb, blend),
                    BooleanOp::Difference => -smooth_min(-va, vb, blend),
                    BooleanOp::Intersection => -smooth_min(-va, -vb, blend),
                };

                let is_active = match op {
                    BooleanOp::Union => active_a || active_b,
                    BooleanOp::Difference => {
                        // Active if near A's surface or near B's cutting surface inside A
                        active_a || (active_b && va < 0.0)
                    }
                    BooleanOp::Intersection => {
                        // Active where both are relevant
                        (active_a && vb <= 0.0) || (active_b && va <= 0.0) || (active_a && active_b)
                    }
                };

                let is_inside = v_out < 0.0;
                let is_near_surface = is_active && v_out < combined_bg;

                if is_inside || is_near_surface {
                    out_leaf.set(offset, v_out.clamp(-combined_bg, combined_bg));
                    active_count += 1;
                } else {
                    out_leaf.values[offset] = combined_bg;
                    out_leaf.mask.set_off(offset);
                }
            }

            if active_count > 0 { Some((orig, out_leaf)) } else { None }
        })
        .collect();

    target.background = combined_bg;
    target.leaves = updated_leaves.into_iter().collect();
}

/// Union of two VDB level sets in place: `a = a ∪ b`.
pub fn csg_union(a: &mut VdbFloatGrid, b: &VdbFloatGrid) {
    csg_operation(a, b, BooleanOp::Union);
}

/// Difference of two VDB level sets in place: `a = a \ b`.
pub fn csg_difference(a: &mut VdbFloatGrid, b: &VdbFloatGrid) {
    csg_operation(a, b, BooleanOp::Difference);
}

/// Intersection of two VDB level sets in place: `a = a ∩ b`.
pub fn csg_intersection(a: &mut VdbFloatGrid, b: &VdbFloatGrid) {
    csg_operation(a, b, BooleanOp::Intersection);
}
