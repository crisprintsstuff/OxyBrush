// Matcap viewport shader. `//PRIM` lines are enabled only when the GPU
// supports `primitive_index` (exact per-face group colors); otherwise the
// flat per-vertex group color is used.
//PRIM enable primitive_index;

struct Uniforms {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    // y: mask overlay strength, z: viewport aspect
    params: vec4<f32>,
    mask_color: vec4<f32>,
    bg_center: vec4<f32>,
    bg_edge: vec4<f32>,
};

struct Piece {
    // x: show face groups, y: 1 = active piece
    flags: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var matcap_tex: texture_2d<f32>;
@group(0) @binding(2) var matcap_sampler: sampler;
@group(1) @binding(0) var<uniform> piece: Piece;
//PRIM @group(1) @binding(1) var<storage, read> face_colors: array<u32>;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) mask: f32,
    @location(3) group_color: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_normal: vec3<f32>,
    @location(1) mask: f32,
    @location(2) @interpolate(flat) group_color: u32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.view_normal = (u.view * vec4<f32>(v.normal, 0.0)).xyz;
    o.mask = v.mask;
    o.group_color = v.group_color;
    return o;
}

fn unpack(c: u32) -> vec3<f32> {
    return vec3<f32>(f32(c & 255u), f32((c >> 8u) & 255u), f32((c >> 16u) & 255u)) / 255.0;
}

@fragment
fn fs_main(
    i: VsOut,
    @builtin(front_facing) front: bool,
    //PRIM @builtin(primitive_index) prim: u32,
) -> @location(0) vec4<f32> {
    var n = normalize(i.view_normal);
    if (!front) {
        n = -n;
    }
    let uv = vec2<f32>(n.x * 0.49 + 0.5, 0.5 - n.y * 0.49);
    var color = textureSample(matcap_tex, matcap_sampler, uv).rgb;
    if (piece.flags.x > 0.5) {
        var gc = i.group_color;
        //PRIM gc = face_colors[prim];
        let g = unpack(gc);
        // Tint by group color while keeping the matcap's shading.
        let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        color = mix(color, g * lum * 1.6, 0.75);
    }
    if (piece.flags.y < 0.5) {
        // Inactive pieces are slightly desaturated and darker.
        let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        color = mix(color, vec3<f32>(lum), 0.6) * 0.75;
    }
    if (!front) {
        color = color * vec3<f32>(0.35, 0.35, 0.45);
    }
    let m = clamp(i.mask, 0.0, 1.0) * u.params.y;
    color = mix(color, color * u.mask_color.rgb, m);
    return vec4<f32>(color, 1.0);
}

// Viewport background: a soft radial gradient, slightly above center, so
// the model reads well against it.
struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) p: vec2<f32>,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BgOut {
    // Fullscreen triangle covering the viewport.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: BgOut;
    o.clip = vec4<f32>(p, 1.0, 1.0);
    o.p = p;
    return o;
}

@fragment
fn fs_background(i: BgOut) -> @location(0) vec4<f32> {
    let d = length(vec2<f32>(i.p.x * max(u.params.z, 0.1) * 0.7, i.p.y - 0.2));
    let t = smoothstep(0.0, 1.35, d);
    return vec4<f32>(mix(u.bg_center.rgb, u.bg_edge.rgb, t), 1.0);
}
