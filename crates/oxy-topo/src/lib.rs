//! Topology operations that rebuild a mesh: voxel remeshing (including
//! unions of several meshes) and quadric decimation. Pure Rust, no C/C++
//! toolchain required.

pub mod decimate;
pub mod print3d;
pub mod remesh;
pub mod transfer;

pub use decimate::decimate;
pub use oxy_vdb::BooleanOp;
pub use print3d::{
    DrainHole, HollowOptions, PrintDimensions, SplitAxis, SplitOptions, WallThicknessReport, analyze_wall_thickness,
    calculate_print_dimensions, check_watertight, hollow_mesh, split_with_keys,
};
pub use remesh::{
    MAX_RESOLUTION, RemeshEngine, RemeshError, RemeshOptions, VdbRemeshOptions, boolean_difference,
    boolean_intersection, boolean_mesh, boolean_union, regular_grid_remesh, vdb_remesh, voxel_remesh,
};
pub use transfer::transfer_attributes;
