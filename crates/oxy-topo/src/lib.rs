//! Topology operations that rebuild a mesh: voxel remeshing (including
//! unions of several meshes) and quadric decimation. Pure Rust, no C/C++
//! toolchain required.

pub mod decimate;
pub mod remesh;

pub use decimate::decimate;
pub use oxy_vdb::BooleanOp;
pub use remesh::{
    MAX_RESOLUTION, RemeshEngine, RemeshError, RemeshOptions, VdbRemeshOptions, boolean_difference,
    boolean_intersection, boolean_mesh, boolean_union, regular_grid_remesh, vdb_remesh, voxel_remesh,
};
