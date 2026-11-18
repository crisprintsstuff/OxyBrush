//! Constructive Solid Geometry (CSG) volumetric boolean operations on VDB level sets.
//!
//! Uses exact R-function algebra on signed distance fields (Pasko et al., Function Representation in Geometric Modeling):
//! - Union: target ∪ tool = min(d_A, d_B)
//! - Difference: target \ tool = max(d_A, -d_B)
//! - Intersection: target ∩ tool = max(d_A, d_B)

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
    let _ = csg_operation_seam(target, tool, op, 0.0);
}

/// [`csg_operation`], also returning the weld: voxels where both operands are
/// active and within `seam_band` (world units) of their own surfaces, and the
/// combined field is too. That is the curve the two surfaces meet along.
/// `seam_band <= 0` marks nothing (what [`csg_operation`] asks for).
///
/// The band is how a boolean tells its weld from the rest of the shape, so it
/// can relax only there. A hard edge of one input, away from the other
/// surface, is not in the band.
pub fn csg_operation_seam(target: &mut VdbFloatGrid, tool: &VdbFloatGrid, op: BooleanOp, seam_band: f32) -> Vec<IVec3> {
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
    let mark_seam = seam_band > 0.0;

    let updated_leaves: Vec<(IVec3, LeafNode, Vec<IVec3>)> = origins_vec
        .into_par_iter()
        .filter_map(|orig| {
            let leaf_a = target_leaves.get(&orig);
            let leaf_b = tool_leaves.get(&orig);

            let mut out_leaf = LeafNode::new(orig, combined_bg);
            let mut active_count = 0;
            let mut seam = Vec::new();

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

                let is_inside = v_out < 0.0;
                let is_near_surface = is_active && v_out < combined_bg;

                if is_inside || is_near_surface {
                    out_leaf.set(offset, v_out.clamp(-combined_bg, combined_bg));
                    active_count += 1;
                } else {
                    out_leaf.values[offset] = combined_bg;
                    out_leaf.mask.set_off(offset);
                }

                // Both surfaces nearby, and the result too: the weld, not a
                // face that belongs to only one operand.
                if mark_seam
                    && active_a
                    && active_b
                    && va.abs() <= seam_band
                    && vb.abs() <= seam_band
                    && v_out.abs() <= seam_band
                {
                    let x = (offset & 7) as i32;
                    let y = ((offset >> 3) & 7) as i32;
                    let z = ((offset >> 6) & 7) as i32;
                    seam.push(orig + IVec3::new(x, y, z));
                }
            }

            if active_count > 0 { Some((orig, out_leaf, seam)) } else { None }
        })
        .collect();

    target.background = combined_bg;
    let mut seam = Vec::new();
    target.leaves = updated_leaves
        .into_iter()
        .map(|(orig, leaf, part)| {
            seam.extend(part);
            (orig, leaf)
        })
        .collect();
    seam
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
