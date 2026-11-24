struct Params {
    corners: array<vec4<f32>, 4>,
    opacity: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var image_sampler: sampler;

struct Vertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Vertex {
    let corner = array<u32, 6>(0u, 2u, 1u, 1u, 2u, 3u)[i];
    var out: Vertex;
    out.clip = params.corners[corner];
    out.uv = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));
    return out;
}

struct Pixel {
    @location(0) color: vec4<f32>,
    @location(1) normal_depth: vec4<f32>,
};

@fragment
fn fs_main(v: Vertex) -> Pixel {
    let color = textureSample(image, image_sampler, v.uv);
    return Pixel(vec4<f32>(color.rgb, color.a * params.opacity.x), vec4<f32>(0.0));
}
