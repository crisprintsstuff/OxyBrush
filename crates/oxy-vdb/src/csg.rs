//! Constructive Solid Geometry (CSG) volumetric boolean operations on VDB level sets.
//!
//! Mirrors `openvdb::tools::csgUnion`, `csgDifference`, and `csgIntersection`.

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
                    BooleanOp::Union => va.min(vb),
                    BooleanOp::Difference => va.max(-vb),
                    BooleanOp::Intersection => va.max(vb),
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

                if is_active && v_out.abs() <= combined_bg {
                    out_leaf.set(offset, v_out);
                    active_count += 1;
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
