// Depth-only pass for shadow maps: the scene seen from a light.
@group(0) @binding(0) var<uniform> light_view_proj: mat4x4<f32>;

@vertex
fn vs_shadow(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return light_view_proj * vec4<f32>(position, 1.0);
}

// A copy (surface instances): its model matrix first.
@vertex
fn vs_shadow_instanced(
    @location(0) position: vec3<f32>,
    @location(5) m0: vec4<f32>,
    @location(6) m1: vec4<f32>,
    @location(7) m2: vec4<f32>,
    @location(8) m3: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(m0, m1, m2, m3);
    return light_view_proj * (model * vec4<f32>(position, 1.0));
}
