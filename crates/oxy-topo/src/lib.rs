//! Topology operations that rebuild a mesh: voxel remeshing (including
//! unions of several meshes), OxyRemesh (field-aligned auto-retopology), and quadric
//! decimation. Pure Rust, no C/C++ toolchain required.

pub mod auto_retopo;
pub mod decimate;
pub mod isoline_extract;
pub mod print3d;
pub mod quad_skin;
pub mod remesh;
pub mod skin;
pub mod transfer;
pub mod uv_cut;
pub mod uv_seams;
pub mod winding;

pub use auto_retopo::{AutoRetopoError, AutoRetopoOptions, OrientationConstraint, auto_retopo};
pub use decimate::decimate;
pub use oxy_vdb::BooleanOp;
pub use print3d::{
    DrainHole, HollowOptions, PrintDimensions, SplitAxis, SplitOptions, WallThicknessReport, analyze_wall_thickness,
    calculate_print_dimensions, check_watertight, hollow_mesh, split_with_keys,
};
pub use quad_skin::{
    DEFAULT_QUAD_RESOLUTION, MAX_QUAD_SUBDIVISION, QuadSkinError, QuadSkinOptions, quad_skin_armature, quad_skin_mesh,
};
pub use remesh::{
    DEFAULT_BOOLEAN_BLEND, MAX_RESOLUTION, RemeshEngine, RemeshError, RemeshOptions, Shell, ShellSide,
    VdbRemeshOptions, boolean_difference, boolean_intersection, boolean_mesh, boolean_union, regular_grid_remesh,
    vdb_remesh, voxel_remesh,
};
pub use skin::{SkinOptions, skin_armature};
pub use transfer::{SurfaceProjector, transfer_attributes};
