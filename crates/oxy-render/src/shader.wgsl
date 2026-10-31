struct Uniforms {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    // x: wireframe-ish edge darkening (unused yet), y: mask overlay strength
    params: vec4<f32>,
    mask_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var matcap_tex: texture_2d<f32>;
@group(0) @binding(2) var matcap_sampler: sampler;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) mask: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_normal: vec3<f32>,
    @location(1) mask: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.view_normal = (u.view * vec4<f32>(v.normal, 0.0)).xyz;
    o.mask = v.mask;
    return o;
}

@fragment
fn fs_main(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var n = normalize(i.view_normal);
    if (!front) {
        n = -n;
    }
    let uv = vec2<f32>(n.x * 0.49 + 0.5, 0.5 - n.y * 0.49);
    var color = textureSample(matcap_tex, matcap_sampler, uv).rgb;
    if (!front) {
        color = color * vec3<f32>(0.35, 0.35, 0.45);
    }
    let m = clamp(i.mask, 0.0, 1.0) * u.params.y;
    color = mix(color, color * u.mask_color.rgb, m);
    return vec4<f32>(color, 1.0);
}
