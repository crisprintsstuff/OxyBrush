struct Uniforms {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    params: vec4<f32>,
    mask_color: vec4<f32>,
    bg_center: vec4<f32>,
    bg_edge: vec4<f32>,
    wireframe_color: vec4<f32>,
};

struct Piece {
    flags: vec4<f32>,
    material: vec4<f32>,
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

struct InstIn {
    @location(5) m0: vec4<f32>,
    @location(6) m1: vec4<f32>,
    @location(7) m2: vec4<f32>,
    @location(8) m3: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
};

// How far an edge is drawn in front of its faces, as a share of its
// distance from the camera, so the lines don't flicker into the surface.
const LIFT: f32 = 0.006;

// `p` moved towards the camera by LIFT of its depth, along the line of
// sight, so it lands on the same pixel. A fixed nudge in depth-buffer units
// (as before) grows with the square of the distance: at the back of a model
// it put hidden edges in front of faces seen at a glancing angle (#417).
fn toward_camera(p: vec3<f32>) -> vec3<f32> {
    // The camera position and forward direction from the view matrix.
    let r0 = u.view[0].xyz;
    let r1 = u.view[1].xyz;
    let r2 = u.view[2].xyz;
    let t = u.view[3].xyz;
    // The view rotation's rows are the camera's axes, so its columns'
    // dot products with the translation give the eye: eye = -Rᵀt.
    let eye = -vec3<f32>(dot(r0, t), dot(r1, t), dot(r2, t));
    let forward = -vec3<f32>(r0.z, r1.z, r2.z);
    let depth = dot(p - eye, forward);
    // Orthographic: every line of sight is `forward`. Perspective: through the eye.
    let ortho = abs((u.view_proj * vec4<f32>(eye, 1.0)).w) > 0.5;
    if ortho {
        return p - forward * (depth * LIFT);
    }
    return eye + (p - eye) * (1.0 - LIFT);
}

@vertex
fn vs_wireframe(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(toward_camera(v.position), 1.0);
    return o;
}

// A copy (surface instances): its model matrix first.
@vertex
fn vs_wireframe_instanced(v: VsIn, inst: InstIn) -> VsOut {
    var o: VsOut;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    o.clip = u.view_proj * vec4<f32>(toward_camera((model * vec4<f32>(v.position, 1.0)).xyz), 1.0);
    return o;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal_depth: vec4<f32>,
};

@fragment
fn fs_wireframe() -> FsOut {
    let opacity = clamp(piece.flags.w, 0.0, 1.0);
    return FsOut(
        vec4<f32>(u.wireframe_color.rgb, opacity),
        vec4<f32>(0.0, 0.0, 0.0, 0.0),
    );
}
