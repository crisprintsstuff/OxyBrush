//! Topology operations that rebuild a mesh: voxel remeshing (including
//! unions of several meshes) and quadric decimation. Pure Rust, no C/C++
//! toolchain required.

pub mod decimate;
pub mod remesh;

pub use decimate::decimate;
pub use remesh::{MAX_RESOLUTION, RemeshError, RemeshOptions, voxel_remesh};
