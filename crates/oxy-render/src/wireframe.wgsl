struct Uniforms {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    params: vec4<f32>,
    mask_color: vec4<f32>,
    bg_center: vec4<f32>,
    bg_edge: vec4<f32>,
};

struct Piece {
    flags: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var<uniform> piece: Piece;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) mask: f32,
    @location(3) group_color: u32,
    @location(4) paint: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_wireframe(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    return o;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal_depth: vec4<f32>,
};

@fragment
fn fs_wireframe() -> FsOut {
    let opacity = clamp(piece.flags.w, 0.0, 1.0);
    // Dark neutral wireframe with subtle slate tint
    return FsOut(
        vec4<f32>(0.10, 0.12, 0.16, opacity),
        vec4<f32>(0.0, 0.0, 0.0, 0.0),
    );
}
