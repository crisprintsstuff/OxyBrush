// Matcap viewport shader. `//PRIM` lines are enabled only when the GPU
// supports `primitive_index` (exact per-face group colors); otherwise the
// flat per-vertex group color is used.
//PRIM enable primitive_index;

struct Uniforms {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    // x: flat shading, y: mask overlay strength, z: viewport aspect,
    // w: flat background (1) or gradient (0)
    params: vec4<f32>,
    mask_color: vec4<f32>,
    bg_center: vec4<f32>,
    bg_edge: vec4<f32>,
    wireframe_color: vec4<f32>,
    // Floor grid: height, cell size, fade radius, opacity (0 = off).
    floor: vec4<f32>,
    // Floor grid: center x, center z.
    floor_center: vec4<f32>,
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
    @location(4) paint: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) view_normal: vec3<f32>,
    @location(1) mask: f32,
    @location(2) @interpolate(flat) group_color: u32,
    @location(3) view_depth: f32,
    @location(4) view_pos: vec3<f32>,
    @location(5) paint: vec3<f32>,
};

// Two targets: the shaded color, and view normal (xyz) + linear depth (w)
// for the screen-space shading passes (shading.wgsl).
struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal_depth: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.view_normal = (u.view * vec4<f32>(v.normal, 0.0)).xyz;
    o.mask = v.mask;
    o.group_color = v.group_color;
    let vp = (u.view * vec4<f32>(v.position, 1.0)).xyz;
    o.view_depth = -vp.z;
    o.view_pos = vp;
    // Paint is stored as the sRGB color the user picked; shade in linear.
    o.paint = pow(unpack(v.paint), vec3<f32>(2.2));
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
) -> FsOut {
    var n = normalize(i.view_normal);
    if (!front) {
        n = -n;
    }
    if (u.params.x > 0.5) {
        // Flat shading: the triangle's own normal from screen-space
        // derivatives of the view position. It always faces the viewer, so
        // back faces need no flip.
        n = normalize(cross(dpdy(i.view_pos), dpdx(i.view_pos)));
    }
    let uv = vec2<f32>(n.x * 0.49 + 0.5, 0.5 - n.y * 0.49);
    var color = textureSample(matcap_tex, matcap_sampler, uv).rgb;
    // Vertex color: tint by the paint while keeping the material's shading.
    // White (unpainted) leaves the material exactly as it is.
    let paint_w = clamp(distance(i.paint, vec3<f32>(1.0)) * 4.0, 0.0, 1.0);
    if (paint_w > 0.0) {
        let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        color = mix(color, i.paint * lum * 1.6, paint_w);
    }
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
    var out: FsOut;
    out.color = vec4<f32>(color, 1.0);
    out.normal_depth = vec4<f32>(n, i.view_depth);
    return out;
}

// Viewport background. A gradient is a soft radial falloff, slightly above
// center, so the model reads well against it. Flat uses the edge color alone.
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
fn fs_background(i: BgOut) -> FsOut {
    let d = length(vec2<f32>(i.p.x * max(u.params.z, 0.1) * 0.7, i.p.y - 0.2));
    // params.w > 0.5 forces t = 1, so the backdrop is the edge color only.
    // With the flag off this is the same smoothstep as before.
    let t = select(smoothstep(0.0, 1.35, d), 1.0, u.params.w > 0.5);
    var out: FsOut;
    out.color = vec4<f32>(mix(u.bg_center.rgb, u.bg_edge.rgb, t), 1.0);
    out.normal_depth = vec4<f32>(0.0);
    return out;
}

// ---- Floor grid ----

struct FloorOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
};

@vertex
fn vs_floor(@builtin(vertex_index) i: u32) -> FloorOut {
    // Two triangles covering the grid's square.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let xz = u.floor_center.xy + corners[i] * u.floor.z;
    var o: FloorOut;
    o.clip = u.view_proj * vec4<f32>(xz.x, u.floor.x, xz.y, 1.0);
    o.world = xz;
    return o;
}

// Antialiased grid lines: distance to the nearest line in pixels.
fn grid_line(p: vec2<f32>, cell: f32, width: f32) -> f32 {
    let g = p / cell;
    let d = abs(fract(g - 0.5) - 0.5) / max(fwidth(g), vec2<f32>(1e-6));
    return 1.0 - clamp(min(d.x, d.y) - (width - 1.0) * 0.5, 0.0, 1.0);
}

@fragment
fn fs_floor(i: FloorOut) -> FsOut {
    let cell = u.floor.y;
    let minor = grid_line(i.world, cell, 1.0);
    let major = grid_line(i.world, cell * 10.0, 1.3);
    // Fade out towards the edge of the grid and where minor lines get
    // denser than a few pixels (far away or at grazing angles).
    let r = length(i.world - u.floor_center.xy) / u.floor.z;
    let edge = 1.0 - smoothstep(0.45, 1.0, r);
    let density = 1.0 - smoothstep(0.15, 0.45, max(fwidth(i.world.x), fwidth(i.world.y)) / cell);
    let a = max(minor * 0.10 * density, major * 0.22) * edge * u.floor.w;
    // Light lines on dark backgrounds, dark lines on light ones.
    let bg = dot(u.bg_center.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let line = select(vec3<f32>(0.5), vec3<f32>(0.1), bg > 0.3);
    var out: FsOut;
    out.color = vec4<f32>(line, a);
    out.normal_depth = vec4<f32>(0.0);
    return out;
}
