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
    // Viewport lights (see lighting.rs, GpuLighting).
    lighting: Lighting,
};

// One light, four vec4s (see lighting.rs, GpuLight):
// a: sun direction or position (view space), w: kind (0 sun, 1 point, 2 spot)
// b: color * intensity, w: falloff exponent
// c: spot direction (view space), w: cos(outer half-angle)
// d: x cos(inner half-angle), y shadow layer (-1 none), z softness (texels), w reference distance
struct Light {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
};

struct Lighting {
    // x: lit (1) or matcap (0), y: light count, z: exposure, w: rim
    params: vec4<f32>,
    // rgb surface color, w roughness
    surface: vec4<f32>,
    // rgb sky light, w specular
    sky: vec4<f32>,
    // rgb ground light, w shadow map size
    ground: vec4<f32>,
    // xyz view-space up, w: orthographic (1)
    up: vec4<f32>,
    lights: array<Light, 4>,
    shadow_matrices: array<mat4x4<f32>, 4>,
};

struct Piece {
    // x: show face groups, y: 1 = active piece
    flags: vec4<f32>,
    // x: matcap layer (0: the scene's matcap, 1+: a piece material)
    material: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var matcap_tex: texture_2d_array<f32>;
@group(0) @binding(2) var matcap_sampler: sampler;
@group(0) @binding(3) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(4) var shadow_sampler: sampler_comparison;
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
    @location(6) world_pos: vec3<f32>,
    @location(7) world_normal: vec3<f32>,
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
    o.world_pos = v.position;
    o.world_normal = v.normal;
    return o;
}

const PI: f32 = 3.14159265;

// Points on a disk for soft shadow edges (a 12-sample Poisson-like set).
const DISK = array<vec2<f32>, 12>(
    vec2<f32>(-0.326, -0.406), vec2<f32>(-0.840, -0.074), vec2<f32>(-0.696, 0.457),
    vec2<f32>(-0.203, 0.621), vec2<f32>(0.962, -0.195), vec2<f32>(0.473, -0.480),
    vec2<f32>(0.519, 0.767), vec2<f32>(0.185, -0.893), vec2<f32>(0.507, 0.064),
    vec2<f32>(0.896, 0.412), vec2<f32>(-0.322, -0.933), vec2<f32>(-0.792, -0.598),
);

// How much of a light reaches `wp` (1 = all), from its shadow map.
fn shadow_factor(light: Light, wp: vec3<f32>, wn: vec3<f32>, n_dot_l: f32) -> f32 {
    let layer = i32(light.d.y + 0.5);
    if (light.d.y < -0.5) {
        return 1.0;
    }
    let size = max(u.lighting.ground.w, 1.0);
    let m = u.lighting.shadow_matrices[layer];
    // Push the lookup off the surface a little (more at grazing angles), so
    // a surface doesn't shadow itself.
    let probe = m * vec4<f32>(wp, 1.0);
    let texel_world = 2.0 / (size * max(length(m[0].xyz), 1e-6));
    let offset = wn * texel_world * (1.5 + 2.0 * (1.0 - n_dot_l));
    let clip = m * vec4<f32>(wp + offset, 1.0);
    if (clip.w <= 0.0 || probe.w <= 0.0) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z > 1.0) {
        return 1.0;
    }
    let depth = ndc.z - 0.0005;
    let radius = light.d.z / size;
    var lit = 0.0;
    for (var k = 0; k < 12; k++) {
        lit += textureSampleCompareLevel(shadow_maps, shadow_sampler, uv + DISK[k] * radius, layer, depth);
    }
    return lit / 12.0;
}

// The surface under the viewport lights: diffuse plus GGX specular
// (Walter et al., "Microfacet Models for Refraction through Rough
// Surfaces", EGSR 2007) with Schlick's Fresnel and Smith-Schlick
// visibility, sky/ground light and a rim, tone mapped with Narkowicz's
// fit of the ACES curve (2015).
fn lit_color(n: vec3<f32>, vp: vec3<f32>, wp: vec3<f32>, wn_in: vec3<f32>, front: bool) -> vec3<f32> {
    let L = u.lighting;
    var view_dir = normalize(-vp);
    if (L.up.w > 0.5) {
        view_dir = vec3<f32>(0.0, 0.0, 1.0);
    }
    var wn = normalize(wn_in);
    if (!front) {
        wn = -wn;
    }
    let base = L.surface.rgb;
    let rough = clamp(L.surface.w, 0.04, 1.0);
    let alpha = rough * rough;
    let f0 = vec3<f32>(mix(0.02, 0.08, L.sky.w));
    let n_dot_v = max(dot(n, view_dir), 1e-4);
    let hemi = dot(n, L.up.xyz) * 0.5 + 0.5;
    var color = mix(L.ground.rgb, L.sky.rgb, hemi) * base;
    let count = i32(L.params.y + 0.5);
    for (var i = 0; i < 4; i++) {
        if (i >= count) {
            break;
        }
        let light = L.lights[i];
        var to_light: vec3<f32>;
        var atten = 1.0;
        if (light.a.w < 0.5) {
            to_light = normalize(light.a.xyz);
        } else {
            let d = light.a.xyz - vp;
            let dist = max(length(d), 1e-5);
            to_light = d / dist;
            atten = min(pow(light.d.w / dist, light.b.w), 64.0);
            if (light.a.w > 1.5) {
                let c = dot(-to_light, normalize(light.c.xyz));
                atten *= smoothstep(light.c.w, max(light.d.x, light.c.w + 1e-4), c);
            }
        }
        let n_dot_l = dot(n, to_light);
        if (n_dot_l <= 0.0 || atten <= 0.0) {
            continue;
        }
        let shadow = shadow_factor(light, wp, wn, n_dot_l);
        let h = normalize(to_light + view_dir);
        let n_dot_h = max(dot(n, h), 0.0);
        let a2 = alpha * alpha;
        let dd = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
        let ndf = a2 / (PI * dd * dd);
        let k = alpha * 0.5;
        let vis = 1.0 / ((n_dot_l * (1.0 - k) + k) * (n_dot_v * (1.0 - k) + k));
        let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, view_dir), 0.0), 5.0);
        let spec = ndf * vis * 0.25 * fresnel * L.sky.w * 4.0;
        color += (base * (1.0 - fresnel) + spec) * light.b.rgb * n_dot_l * atten * shadow;
    }
    // Rim: edges turning away from the viewer catch a little sky light.
    let rim = pow(1.0 - n_dot_v, 3.0) * L.params.w;
    color += rim * mix(L.sky.rgb, vec3<f32>(1.0), 0.5);
    let x = color * L.params.z;
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
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
    // Sampled in uniform control flow either way, then replaced when lit.
    var color = textureSample(matcap_tex, matcap_sampler, uv, i32(piece.material.x + 0.5)).rgb;
    if (u.lighting.params.x > 0.5) {
        color = lit_color(n, i.view_pos, i.world_pos, i.world_normal, front);
    }
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
    if (piece.flags.y > 1.5) {
        // Previews (not yet in the scene) get a cool blue glaze that keeps
        // the matcap's shading, so they don't read as part of the model.
        let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        color = mix(color, vec3<f32>(0.42, 0.66, 1.0) * (0.35 + lum), 0.6);
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
