//! 3D-printing preparation tools: real-world metrics (dimensions, volume, weight),
//! wall-thickness analysis, hollowing with drain holes, and piece splitting with alignment keys.

use crate::remesh::RemeshError;
use glam::{Quat, Vec3};
use oxy_mesh::{Mesh, primitives};
use oxy_vdb::MeshToVolumeOptions;
use rayon::prelude::*;

/// Real-world physical dimensions and material consumption metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintDimensions {
    /// Bounding box dimensions in millimeters (width = X, height = Y, depth = Z).
    pub size_mm: Vec3,
    /// Surface area in square centimeters.
    pub surface_area_cm2: f32,
    /// Watertight volume in cubic centimeters.
    pub volume_cm3: f32,
    /// Whether the mesh is a closed watertight manifold.
    pub is_watertight: bool,
    /// Estimated UV-curing photopolymer resin weight in grams (~1.15 g/cm³).
    pub resin_weight_grams: f32,
    /// Estimated PLA filament weight in grams (~1.24 g/cm³).
    pub pla_weight_grams: f32,
}

/// Calculate real-world print metrics for a mesh given `unit_scale` (mm per world unit).
pub fn calculate_print_dimensions(mesh: &Mesh, unit_scale: f32) -> PrintDimensions {
    let bounds = mesh.bounds();
    let size_world = bounds.extent();
    let size_mm = size_world * unit_scale;

    // Surface area: sum of triangle areas
    let mut area_world = 0.0f32;
    // Watertight volume via divergence theorem: sum of (v0 . (v1 x v2)) / 6.0
    let mut signed_vol_world = 0.0f32;

    for &tri in &mesh.triangles {
        let v0 = mesh.positions[tri[0] as usize];
        let v1 = mesh.positions[tri[1] as usize];
        let v2 = mesh.positions[tri[2] as usize];

        let cross = (v1 - v0).cross(v2 - v0);
        area_world += 0.5 * cross.length();
        signed_vol_world += v0.dot(v1.cross(v2)) / 6.0;
    }

    // Convert from world units to cm and cm² / cm³
    // unit_scale is mm per unit, so cm per unit is (unit_scale / 10.0)
    let cm_per_unit = unit_scale / 10.0;
    let surface_area_cm2 = area_world * (cm_per_unit * cm_per_unit);
    let volume_cm3 = signed_vol_world.abs() * (cm_per_unit * cm_per_unit * cm_per_unit);

    let is_watertight = check_watertight(mesh);

    let resin_weight_grams = volume_cm3 * 1.15;
    let pla_weight_grams = volume_cm3 * 1.24;

    PrintDimensions { size_mm, surface_area_cm2, volume_cm3, is_watertight, resin_weight_grams, pla_weight_grams }
}

/// Checks whether a mesh is a topologically closed 2-manifold with no open boundary edges
/// and no non-manifold edges.
pub fn check_watertight(mesh: &Mesh) -> bool {
    if mesh.triangle_count() < 4 {
        return false;
    }
    use std::collections::HashMap;
    let mut half_edges: HashMap<(u32, u32), u32> = HashMap::with_capacity(mesh.triangles.len() * 3);
    for &[a, b, c] in &mesh.triangles {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            *half_edges.entry((u, v)).or_insert(0) += 1;
        }
    }
    // In a closed orientable 2-manifold, every directed edge (u, v) occurs exactly once,
    // and its opposite edge (v, u) also occurs exactly once.
    for (&(u, v), &count) in &half_edges {
        if count != 1 {
            return false;
        }
        if half_edges.get(&(v, u)).copied().unwrap_or(0) != 1 {
            return false;
        }
    }
    true
}

/// Wall-thickness analysis report.
#[derive(Debug, Clone)]
pub struct WallThicknessReport {
    /// Minimum detected wall thickness in millimeters.
    pub min_thickness_mm: f32,
    /// Average detected wall thickness in millimeters across the surface.
    pub avg_thickness_mm: f32,
    /// Ratio of vertices whose local wall thickness is below `min_threshold_mm` (0.0 to 1.0).
    pub thin_ratio: f32,
    /// Per-vertex thickness in millimeters (parallel to `mesh.positions`).
    pub vertex_thickness_mm: Vec<f32>,
}

/// Analyze local wall thickness of `mesh` by inward raycasting along negative normals.
pub fn analyze_wall_thickness(
    mesh: &Mesh,
    unit_scale: f32,
    min_threshold_mm: f32,
    max_check_dist_mm: f32,
) -> WallThicknessReport {
    let n_verts = mesh.vertex_count();
    if n_verts == 0 {
        return WallThicknessReport {
            min_thickness_mm: 0.0,
            avg_thickness_mm: 0.0,
            thin_ratio: 0.0,
            vertex_thickness_mm: Vec::new(),
        };
    }

    let max_check_world = (max_check_dist_mm / unit_scale).max(1e-4);
    let epsilon = 1e-3f32; // Small inward offset to prevent self-intersection

    // Parallel raycast for each vertex
    let vertex_thickness_mm: Vec<f32> = (0..n_verts)
        .into_par_iter()
        .map(|i| {
            let pos = mesh.positions[i];
            let norm = mesh.normals[i];
            if norm.length_squared() < 1e-6 {
                return max_check_dist_mm;
            }

            let origin = pos - norm * epsilon;
            let dir = -norm;

            if let Some(hit) = mesh.raycast(origin, dir)
                && hit.t > 0.0
                && hit.t < max_check_world
            {
                let thick_world = hit.t + epsilon;
                return thick_world * unit_scale;
            }
            max_check_dist_mm
        })
        .collect();

    let mut min_t = f32::MAX;
    let mut sum_t = 0.0f32;
    let mut thin_count = 0usize;

    for &t in &vertex_thickness_mm {
        if t < min_t {
            min_t = t;
        }
        sum_t += t;
        if t < min_threshold_mm {
            thin_count += 1;
        }
    }

    let avg_thickness_mm = sum_t / n_verts as f32;
    let thin_ratio = thin_count as f32 / n_verts as f32;

    WallThicknessReport {
        min_thickness_mm: if min_t == f32::MAX { max_check_dist_mm } else { min_t },
        avg_thickness_mm,
        thin_ratio,
        vertex_thickness_mm,
    }
}

/// Drain hole specification for resin or powder evacuation in hollow 3D prints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrainHole {
    /// Center of the drain hole on the outer surface (in world units).
    pub position: Vec3,
    /// Direction pointing through the wall.
    pub direction: Vec3,
    /// Radius of the cylindrical hole in millimeters.
    pub radius_mm: f32,
    /// Length of the hole cutter cylinder in millimeters (must exceed wall thickness).
    pub length_mm: f32,
}

/// Configuration options for hollowing a mesh.
#[derive(Debug, Clone)]
pub struct HollowOptions {
    /// Desired uniform wall thickness in millimeters (e.g. 1.5 - 3.0 mm).
    pub wall_thickness_mm: f32,
    /// Unit scale (millimeters per world unit).
    pub unit_scale: f32,
    /// List of drain holes to pierce through the shell.
    pub drain_holes: Vec<DrainHole>,
    /// Optional voxel resolution override (in world units). If None, chosen from mesh bounds.
    pub voxel_size: Option<f32>,
}

/// Converts a solid mesh into a hollow 3D-printable shell of specified wall thickness,
/// optionally punching cylindrical drain holes through the wall.
pub fn hollow_mesh(mesh: &Mesh, opts: HollowOptions) -> Result<Mesh, RemeshError> {
    if mesh.triangle_count() == 0 {
        return Err(RemeshError::Empty);
    }

    let h = opts.voxel_size.unwrap_or_else(|| {
        let b = mesh.bounds();
        let max_dim = b.extent().max_element();
        (max_dim / 128.0).clamp(0.005, 0.2)
    });

    let wall_world = (opts.wall_thickness_mm / opts.unit_scale).max(h * 0.5);

    let vdb_opts = MeshToVolumeOptions { voxel_size: h, half_width: 3.0 };

    // Convert input mesh to solid level set
    let mut outer_grid = oxy_vdb::mesh_to_level_set(&mesh.positions, &mesh.triangles, vdb_opts);

    // Create cavity grid by eroding the solid volume inward
    let mut cavity_grid = outer_grid.clone();
    oxy_vdb::offset(&mut cavity_grid, -wall_world);

    // Add drain holes if specified
    for hole in &opts.drain_holes {
        let r_world = (hole.radius_mm / opts.unit_scale).max(h * 0.5);
        let l_world = (hole.length_mm / opts.unit_scale).max(wall_world * 2.0);
        let cutter = oriented_cylinder(hole.position, hole.direction, r_world, l_world);
        let cutter_grid = oxy_vdb::mesh_to_level_set(&cutter.positions, &cutter.triangles, vdb_opts);
        // Union drain hole with the interior cavity so both are subtracted together
        oxy_vdb::csg_union(&mut cavity_grid, &cutter_grid);
    }

    // Difference the interior cavity and holes from the outer shell
    oxy_vdb::csg_difference(&mut outer_grid, &cavity_grid);

    // Extract mesh via dual contouring
    let hollowed = oxy_vdb::volume_to_mesh(
        &outer_grid,
        oxy_vdb::VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 },
    )
    .ok_or(RemeshError::NoSurface)?;

    Ok(hollowed)
}

/// Axis for planar splitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    X,
    Y,
    Z,
}

/// Options for splitting a piece into interlocking parts with registration pins and sockets.
#[derive(Debug, Clone)]
pub struct SplitOptions {
    /// Coordinate axis to cut across.
    pub axis: SplitAxis,
    /// Coordinate along the axis where the planar cut occurs (in world units).
    pub offset: f32,
    /// Unit scale (mm per world unit).
    pub unit_scale: f32,
    /// Radius of the alignment registration pin in millimeters (e.g. 3.0 mm).
    pub pin_radius_mm: f32,
    /// Height of the alignment registration pin in millimeters (e.g. 6.0 mm).
    pub pin_height_mm: f32,
    /// Clearance tolerance in millimeters for the female socket (e.g. 0.2 mm) so the 3D printed pin fits smoothly.
    pub clearance_mm: f32,
    /// Optional voxel size for volumetric CSG boolean cut.
    pub voxel_size: Option<f32>,
}

/// Splits a mesh along a plane into Part A (with male alignment pin) and Part B (with matching socket).
pub fn split_with_keys(mesh: &Mesh, opts: SplitOptions) -> Result<(Mesh, Mesh), RemeshError> {
    if mesh.triangle_count() == 0 {
        return Err(RemeshError::Empty);
    }

    let bounds = mesh.bounds();
    let size = bounds.extent();
    let h = opts.voxel_size.unwrap_or_else(|| {
        let max_dim = size.max_element();
        (max_dim / 128.0).clamp(0.005, 0.2)
    });

    let pin_r = (opts.pin_radius_mm / opts.unit_scale).max(h);
    let pin_h = (opts.pin_height_mm / opts.unit_scale).max(h * 2.0);
    let socket_r = ((opts.pin_radius_mm + opts.clearance_mm) / opts.unit_scale).max(pin_r);
    let socket_h = ((opts.pin_height_mm + opts.clearance_mm) / opts.unit_scale).max(pin_h);

    // Center of the cut plane on the mesh
    let mut cut_center = bounds.center();
    let cut_dir = match opts.axis {
        SplitAxis::X => {
            cut_center.x = opts.offset;
            Vec3::X
        }
        SplitAxis::Y => {
            cut_center.y = opts.offset;
            Vec3::Y
        }
        SplitAxis::Z => {
            cut_center.z = opts.offset;
            Vec3::Z
        }
    };

    // Construct half-space cutting boxes
    let pad = size.max_element() * 2.0;
    let (box_pos_min, box_pos_max, box_neg_min, box_neg_max) = match opts.axis {
        SplitAxis::X => (
            Vec3::new(opts.offset, bounds.min.y - pad, bounds.min.z - pad),
            Vec3::new(bounds.max.x + pad, bounds.max.y + pad, bounds.max.z + pad),
            Vec3::new(bounds.min.x - pad, bounds.min.y - pad, bounds.min.z - pad),
            Vec3::new(opts.offset, bounds.max.y + pad, bounds.max.z + pad),
        ),
        SplitAxis::Y => (
            Vec3::new(bounds.min.x - pad, opts.offset, bounds.min.z - pad),
            Vec3::new(bounds.max.x + pad, bounds.max.y + pad, bounds.max.z + pad),
            Vec3::new(bounds.min.x - pad, bounds.min.y - pad, bounds.min.z - pad),
            Vec3::new(bounds.max.x + pad, opts.offset, bounds.max.z + pad),
        ),
        SplitAxis::Z => (
            Vec3::new(bounds.min.x - pad, bounds.min.y - pad, opts.offset),
            Vec3::new(bounds.max.x + pad, bounds.max.y + pad, bounds.max.z + pad),
            Vec3::new(bounds.min.x - pad, bounds.min.y - pad, bounds.min.z - pad),
            Vec3::new(bounds.max.x + pad, bounds.max.y + pad, opts.offset),
        ),
    };

    let box_pos = box_from_extents(box_pos_min, box_pos_max);
    let box_neg = box_from_extents(box_neg_min, box_neg_max);

    // Male registration pin centered on the cut plane and extending into Part B
    let pin_mesh = oriented_cylinder(cut_center - cut_dir * (pin_h * 0.5), cut_dir, pin_r, pin_h);
    // Female registration socket (slightly larger for clearance)
    let socket_mesh = oriented_cylinder(cut_center - cut_dir * (socket_h * 0.5), cut_dir, socket_r, socket_h);

    let vdb_opts = MeshToVolumeOptions { voxel_size: h, half_width: 3.0 };

    // Base mesh level set
    let base_grid = oxy_vdb::mesh_to_level_set(&mesh.positions, &mesh.triangles, vdb_opts);
    let box_pos_grid = oxy_vdb::mesh_to_level_set(&box_pos.positions, &box_pos.triangles, vdb_opts);
    let box_neg_grid = oxy_vdb::mesh_to_level_set(&box_neg.positions, &box_neg.triangles, vdb_opts);
    let pin_grid = oxy_vdb::mesh_to_level_set(&pin_mesh.positions, &pin_mesh.triangles, vdb_opts);
    let socket_grid = oxy_vdb::mesh_to_level_set(&socket_mesh.positions, &socket_mesh.triangles, vdb_opts);

    // Part A: Positive half + alignment pin
    let mut part_a_grid = base_grid.clone();
    oxy_vdb::csg_intersection(&mut part_a_grid, &box_pos_grid);
    oxy_vdb::csg_union(&mut part_a_grid, &pin_grid);

    // Part B: Negative half - alignment socket
    let mut part_b_grid = base_grid;
    oxy_vdb::csg_intersection(&mut part_b_grid, &box_neg_grid);
    oxy_vdb::csg_difference(&mut part_b_grid, &socket_grid);

    let mesh_a = oxy_vdb::volume_to_mesh(
        &part_a_grid,
        oxy_vdb::VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 },
    )
    .ok_or(RemeshError::NoSurface)?;

    let mesh_b = oxy_vdb::volume_to_mesh(
        &part_b_grid,
        oxy_vdb::VolumeToMeshOptions { isovalue: 0.0, adaptivity: 0.0, smooth_passes: 1 },
    )
    .ok_or(RemeshError::NoSurface)?;

    Ok((mesh_a, mesh_b))
}

/// Helper to build an oriented cylinder primitive centered at `center` along `direction`.
fn oriented_cylinder(center: Vec3, direction: Vec3, radius: f32, height: f32) -> Mesh {
    let base = primitives::cylinder(16, radius, height);
    let dir = direction.normalize_or_zero();
    let rot = if dir.length_squared() > 1e-4 { Quat::from_rotation_arc(Vec3::Y, dir) } else { Quat::IDENTITY };

    let pos: Vec<Vec3> = base.positions.iter().map(|&p| rot * p + center).collect();
    let norm: Vec<Vec3> = base.normals.iter().map(|&n| rot * n).collect();

    let mut out = Mesh::new(pos, base.triangles);
    out.normals = norm;
    out
}

/// Helper to build a closed box mesh between `min` and `max`.
fn box_from_extents(min: Vec3, max: Vec3) -> Mesh {
    let size = max - min;
    let center = (min + max) * 0.5;
    let base = primitives::cube(1, 1.0);
    let pos: Vec<Vec3> = base.positions.iter().map(|&p| p * size + center).collect();
    Mesh::new(pos, base.triangles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_dimensions_sphere() {
        let sphere = primitives::icosphere(3, 1.0);
        let dims = calculate_print_dimensions(&sphere, 10.0); // 1 unit = 10mm => radius 10mm, diameter 20mm
        assert!((dims.size_mm.x - 20.0).abs() < 1.0);
        assert!((dims.size_mm.y - 20.0).abs() < 1.0);
        assert!((dims.size_mm.z - 20.0).abs() < 1.0);
        assert!(dims.is_watertight);

        // Sphere volume: 4/3 * pi * r^3 where r = 1.0 cm => ~4.188 cm³
        assert!((dims.volume_cm3 - 4.188).abs() < 0.2);
        // Weight: 4.188 * 1.15 => ~4.8g
        assert!((dims.resin_weight_grams - 4.8).abs() < 0.5);
    }

    #[test]
    fn test_wall_thickness_analyzer() {
        let sphere = primitives::icosphere(2, 1.0);
        // On a solid sphere of radius 1.0 (with unit_scale = 10mm => radius 10mm, diameter 20mm),
        // inward raycast from any surface vertex will traverse through the interior to the opposite wall (diameter = ~20mm).
        let report = analyze_wall_thickness(&sphere, 10.0, 5.0, 50.0);
        assert!(report.min_thickness_mm > 15.0);
        assert_eq!(report.thin_ratio, 0.0);
    }

    #[test]
    fn test_hollow_mesh() {
        let sphere = primitives::icosphere(2, 1.0);
        let opts = HollowOptions {
            wall_thickness_mm: 3.0,
            unit_scale: 10.0,
            drain_holes: vec![DrainHole {
                position: Vec3::new(0.0, -1.0, 0.0),
                direction: Vec3::NEG_Y,
                radius_mm: 2.0,
                length_mm: 5.0,
            }],
            voxel_size: Some(0.08),
        };

        let hollowed = hollow_mesh(&sphere, opts).expect("hollowing succeeds");
        assert!(hollowed.triangle_count() > 0);
        // Hollowed mesh has outer and inner surface, so triangle count should exceed original
        assert!(hollowed.triangle_count() >= sphere.triangle_count());
    }

    #[test]
    fn test_split_with_keys() {
        let sphere = primitives::icosphere(2, 1.0);
        let opts = SplitOptions {
            axis: SplitAxis::Y,
            offset: 0.0,
            unit_scale: 10.0,
            pin_radius_mm: 2.0,
            pin_height_mm: 4.0,
            clearance_mm: 0.2,
            voxel_size: Some(0.08),
        };

        let (part_a, part_b) = split_with_keys(&sphere, opts).expect("split with keys succeeds");
        assert!(part_a.triangle_count() > 0);
        assert!(part_b.triangle_count() > 0);
        // Bounds of Part A should be predominantly positive Y
        assert!(part_a.bounds().max.y > 0.5);
        // Bounds of Part B should be predominantly negative Y
        assert!(part_b.bounds().min.y < -0.5);
    }
}
