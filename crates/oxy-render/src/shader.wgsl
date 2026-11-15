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
    // Floor grid: center x, center z; z: show vertex color (1) or not.
    floor_center: vec4<f32>,
    // Viewport lights (see lighting.rs, GpuLighting).
    lighting: Lighting,
};

// One light, five vec4s (see lighting.rs, GpuLight):
// a: sun direction or position (view space), w: kind (0 sun, 1 point, 2 spot, 3 area)
// b: color * intensity, w: falloff exponent
// c: spot/area direction (view space), w: cos(outer half-angle)
// d: x cos(inner half-angle), y shadow layer (-1 none), z softness (texels), w reference distance
// e: x area radius (world), y terminator softness (sine), z/w shadow map near/far
struct Light {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
    e: vec4<f32>,
};

struct Lighting {
    // x: matcap (0), lit (1) or matcap lit by the lights (2), y: light count, z: exposure, w: rim
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
    // x: matcap layer (0: the scene's matcap, 1+: a piece material),
    // y: inactive look (0 off, 1 grey out, 2 darken, 3 tint), z: strength
    material: vec4<f32>,
    // rgb: the Tint look's color (linear)
    inactive_color: vec4<f32>,
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

// An instance's model matrix (surface instances), for the *_instanced
// entry points only: ordinary pieces don't read it.
struct InstIn {
    @location(5) m0: vec4<f32>,
    @location(6) m1: vec4<f32>,
    @location(7) m2: vec4<f32>,
    @location(8) m3: vec4<f32>,
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

// The same for a copy (surface instances): the copy's model matrix first.
@vertex
fn vs_main_instanced(v: VsIn, inst: InstIn) -> VsOut {
    var o: VsOut;
    // Copies scale evenly, so the matrix turns normals too.
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let wp = (model * vec4<f32>(v.position, 1.0)).xyz;
    let wn = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    o.clip = u.view_proj * vec4<f32>(wp, 1.0);
    o.view_normal = (u.view * vec4<f32>(wn, 0.0)).xyz;
    o.mask = v.mask;
    o.group_color = v.group_color;
    let vp = (u.view * vec4<f32>(wp, 1.0)).xyz;
    o.view_depth = -vp.z;
    o.view_pos = vp;
    // Paint is stored as the sRGB color the user picked; shade in linear.
    o.paint = pow(unpack(v.paint), vec3<f32>(2.2));
    o.world_pos = wp;
    o.world_normal = wn;
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

// Linear distance from the light of a perspective shadow map's depth
// (lighting.rs `perspective`: depth = far (d - near) / (d (far - near))).
fn shadow_distance(depth: f32, near: f32, far: f32) -> f32 {
    return far * near / max(far - depth * (far - near), 1e-6);
}

// The inverse: the map's depth at distance `d` from the light.
fn shadow_depth(d: f32, near: f32, far: f32) -> f32 {
    return far * (d - near) / max(d * (far - near), 1e-6);
}

// Sample k of n spread evenly over a unit disk (Vogel's golden-angle
// spiral), turned by `angle`.
fn vogel(k: i32, n: f32, angle: f32) -> vec2<f32> {
    let r = sqrt((f32(k) + 0.5) / n);
    let t = f32(k) * 2.39996323 + angle;
    return vec2<f32>(cos(t), sin(t)) * r;
}

// A point's place on a perspective shadow map: uv, and its distance
// along the map's axis (clip w).
fn shadow_uvd(m: mat4x4<f32>, p: vec3<f32>) -> vec3<f32> {
    let c = m * vec4<f32>(p, 1.0);
    let ndc = c.xy / max(c.w, 1e-6);
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, c.w);
}

// An area light's shadow: percentage-closer soft shadows (Fernando,
// "Percentage-Closer Soft Shadows", NVIDIA 2005). Find the average depth of
// what blocks the light near this point, estimate the penumbra from the
// light's size by similar triangles, then filter over that width: sharp
// where an object touches the surface, softer further away.
fn area_shadow_factor(light: Light, wp: vec3<f32>, wn: vec3<f32>, n_dot_l: f32, pix: vec2<f32>) -> f32 {
    let layer = i32(light.d.y + 0.5);
    if (light.d.y < -0.5) {
        return 1.0;
    }
    let size = max(u.lighting.ground.w, 1.0);
    let m = u.lighting.shadow_matrices[layer];
    let probe = m * vec4<f32>(wp, 1.0);
    if (probe.w <= 0.0) {
        return 1.0;
    }
    // 1 / tan(half the map's field of view): the length of the matrix's first row.
    let f = length(vec3<f32>(m[0].x, m[1].x, m[2].x));
    // World size of one texel at this distance.
    let texel_world = 2.0 * probe.w / (f * size);
    let p = wp + wn * texel_world * 1.5;
    let here = shadow_uvd(m, p);
    let uv = here.xy;
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return 1.0;
    }
    let near = light.e.z;
    let far = light.e.w;
    let d_recv = here.z;
    if (d_recv > far) {
        return 1.0;
    }
    // How this surface's own distance from the light changes across the
    // map (receiver plane depth bias, Isidoro, "Shadow Mapping: GPU-based
    // Tips and Techniques", GDC 2006): each sample is compared with the
    // surface where that sample lands, not with this point, so a wide
    // search doesn't find the surface itself. Found by projecting two
    // nearby points of the tangent plane.
    var t1 = cross(wn, vec3<f32>(0.0, 1.0, 0.0));
    if (dot(t1, t1) < 1e-4) {
        t1 = cross(wn, vec3<f32>(1.0, 0.0, 0.0));
    }
    t1 = normalize(t1);
    let t2 = cross(wn, t1);
    let step = texel_world * 4.0;
    let a = shadow_uvd(m, p + t1 * step) - here;
    let b = shadow_uvd(m, p + t2 * step) - here;
    let det = a.x * b.y - a.y * b.x;
    var grad = vec2<f32>(0.0);
    if (abs(det) > 1e-12) {
        grad = vec2<f32>(a.z * b.y - b.z * a.y, a.x * b.z - b.x * a.z) / det;
    }
    // Seen edge-on the plane is no help: cap the slope.
    let max_slope = d_recv * 8.0;
    if (length(grad) > max_slope) {
        grad = grad / length(grad) * max_slope;
    }
    let bias = texel_world * 2.0;
    let radius = light.e.x;
    // Blocker search: the light's disk as seen from here, on the map.
    let world_to_uv = f / (2.0 * d_recv);
    let search = clamp(radius * world_to_uv, 1.0 / size, 0.08);
    // The sample spiral turned by a different angle at each pixel
    // (interleaved gradient noise, Jimenez, "Next Generation Post Processing
    // in Call of Duty: Advanced Warfare", SIGGRAPH 2014): fine grain instead
    // of the bands a fixed pattern would leave in a wide penumbra.
    let angle = 6.2831853 * fract(52.9829189 * fract(dot(pix, vec2<f32>(0.06711056, 0.00583715))));
    var blockers = 0.0;
    var sum = 0.0;
    let texels = vec2<f32>(size);
    for (var k = 0; k < 12; k++) {
        let o = vogel(k, 12.0, angle) * search;
        let q = clamp((uv + o) * texels, vec2<f32>(0.0), texels - 1.0);
        let d = shadow_distance(textureLoad(shadow_maps, vec2<i32>(q), layer, 0), near, far);
        if (d < d_recv + dot(grad, o) - bias) {
            blockers += 1.0;
            sum += d;
        }
    }
    if (blockers < 0.5) {
        return 1.0;
    }
    // Penumbra width by similar triangles: (receiver - blocker) / blocker,
    // times the light's diameter.
    let d_block = sum / blockers;
    let penumbra = (d_recv - d_block) * 2.0 * radius / max(d_block, 1e-4);
    let filter_r = clamp(penumbra * world_to_uv, light.d.z / size, 0.06);
    var lit = 0.0;
    for (var k = 0; k < 16; k++) {
        let o = vogel(k, 16.0, angle) * filter_r;
        let reference = shadow_depth(d_recv + dot(grad, o) - bias, near, far);
        lit += textureSampleCompareLevel(shadow_maps, shadow_sampler, uv + o, layer, reference);
    }
    return lit / 16.0;
}

// How much light a surface gets when it turns `n_dot_l` toward a light,
// fading out over `h` (a sine) on both sides of the terminator instead of
// stopping there. Exact for a round light of angular radius asin(h) when
// the surface faces it (n_dot_l >= h), and a smooth quadratic through the
// horizon, as for sphere lights in Lagarde and de Rousiers, "Moving
// Frostbite to Physically Based Rendering" (2014).
fn soft_n_dot_l(n_dot_l: f32, h: f32) -> f32 {
    if (n_dot_l >= h) {
        return n_dot_l;
    }
    if (n_dot_l <= -h) {
        return 0.0;
    }
    return (n_dot_l + h) * (n_dot_l + h) / (4.0 * h);
}

// What the lights do to a white surface, before tone mapping: sky and
// ground light, the lights' diffuse (times 1 - Fresnel) and their specular.
struct LightSum {
    ambient: vec3<f32>,
    diffuse: vec3<f32>,
    spec: vec3<f32>,
    n_dot_v: f32,
};

fn light_sum(n: vec3<f32>, vp: vec3<f32>, wp: vec3<f32>, wn_in: vec3<f32>, front: bool, pix: vec2<f32>) -> LightSum {
    let L = u.lighting;
    var view_dir = normalize(-vp);
    if (L.up.w > 0.5) {
        view_dir = vec3<f32>(0.0, 0.0, 1.0);
    }
    var wn = normalize(wn_in);
    if (!front) {
        wn = -wn;
    }
    let rough = clamp(L.surface.w, 0.04, 1.0);
    let alpha = rough * rough;
    let f0 = vec3<f32>(mix(0.02, 0.08, L.sky.w));
    let n_dot_v = max(dot(n, view_dir), 1e-4);
    let hemi = dot(n, L.up.xyz) * 0.5 + 0.5;
    var out: LightSum;
    out.ambient = mix(L.ground.rgb, L.sky.rgb, hemi);
    out.diffuse = vec3<f32>(0.0);
    out.spec = vec3<f32>(0.0);
    out.n_dot_v = n_dot_v;
    let count = i32(L.params.y + 0.5);
    for (var i = 0; i < 4; i++) {
        if (i >= count) {
            break;
        }
        let light = L.lights[i];
        var to_light: vec3<f32>;
        var atten = 1.0;
        var h = light.e.y;
        // Where the highlight comes from, and how much wider it gets.
        var spec_dir: vec3<f32>;
        var spec_norm = 1.0;
        var a_spec = alpha;
        if (light.a.w < 0.5) {
            to_light = normalize(light.a.xyz);
            spec_dir = to_light;
        } else {
            let d = light.a.xyz - vp;
            let dist = max(length(d), 1e-5);
            to_light = d / dist;
            spec_dir = to_light;
            atten = min(pow(light.d.w / dist, light.b.w), 64.0);
            if (light.a.w > 2.5) {
                // Area: a disk facing c.xyz lights the half in front of it,
                // most straight ahead.
                atten *= max(dot(-to_light, normalize(light.c.xyz)), 0.0);
                let radius = light.e.x;
                // The disk as seen from here softens the terminator ...
                h = clamp(max(h, radius / dist), 0.0, 0.95);
                // ... and its highlight is the closest point of the light
                // to the reflected ray, with the lobe widened to keep the
                // energy (the "representative point" of Karis, "Real
                // Shading in Unreal Engine 4", SIGGRAPH 2013).
                let r = reflect(-view_dir, n);
                let to_ray = dot(d, r) * r - d;
                let closest = d + to_ray * clamp(radius / max(length(to_ray), 1e-5), 0.0, 1.0);
                spec_dir = normalize(closest);
                a_spec = clamp(alpha + radius / (2.0 * dist), 0.0, 1.0);
                spec_norm = (alpha / a_spec) * (alpha / a_spec);
            } else if (light.a.w > 1.5) {
                let c = dot(-to_light, normalize(light.c.xyz));
                atten *= smoothstep(light.c.w, max(light.d.x, light.c.w + 1e-4), c);
            }
        }
        let n_dot_l = dot(n, to_light);
        let soft = soft_n_dot_l(n_dot_l, h);
        if (soft <= 0.0 || atten <= 0.0) {
            continue;
        }
        var shadow = 1.0;
        if (light.a.w > 2.5) {
            shadow = area_shadow_factor(light, wp, wn, max(n_dot_l, 0.0), pix);
        } else {
            shadow = shadow_factor(light, wp, wn, max(n_dot_l, 0.0));
        }
        let lit = light.b.rgb * atten * shadow;
        let hv = normalize(spec_dir + view_dir);
        let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(hv, view_dir), 0.0), 5.0);
        out.diffuse += (vec3<f32>(1.0) - fresnel) * lit * soft;
        let s_dot_l = dot(n, spec_dir);
        if (s_dot_l > 0.0) {
            let n_dot_h = max(dot(n, hv), 0.0);
            let a2 = a_spec * a_spec;
            let dd = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
            let ndf = a2 / (PI * dd * dd);
            let k = a_spec * 0.5;
            let vis = 1.0 / ((s_dot_l * (1.0 - k) + k) * (n_dot_v * (1.0 - k) + k));
            let spec = ndf * vis * 0.25 * fresnel * L.sky.w * 4.0 * spec_norm;
            out.spec += spec * lit * s_dot_l;
        }
    }
    return out;
}

// Narkowicz's fit of the ACES tone curve (2015).
fn tone_map(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The matcap lit by the lights (RFC 0024): the matcap stays the material,
// each light brightens it where it shines and its shadows darken it. A
// surface facing the default key light comes out about as the matcap
// alone; where only the sky and fill reach, darker.
fn lit_matcap(matcap: vec3<f32>, s: LightSum) -> vec3<f32> {
    let exposure = u.lighting.params.z;
    let light = (s.ambient + s.diffuse) * exposure * 0.8;
    // Ease highlights above the matcap's own brightness instead of clipping.
    let gain = light / (1.0 + max(light - vec3<f32>(1.0), vec3<f32>(0.0)) * 0.5);
    return clamp(matcap * gain + s.spec * exposure * 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The surface under the viewport lights: diffuse plus GGX specular
// (Walter et al., "Microfacet Models for Refraction through Rough
// Surfaces", EGSR 2007) with Schlick's Fresnel and Smith-Schlick
// visibility, sky/ground light and a rim, tone mapped with Narkowicz's
// fit of the ACES curve (2015).
fn lit_color(n: vec3<f32>, vp: vec3<f32>, wp: vec3<f32>, wn_in: vec3<f32>, front: bool, pix: vec2<f32>) -> vec3<f32> {
    let L = u.lighting;
    let s = light_sum(n, vp, wp, wn_in, front, pix);
    var color = (s.ambient + s.diffuse) * L.surface.rgb + s.spec;
    // Rim: edges turning away from the viewer catch a little sky light.
    let rim = pow(1.0 - s.n_dot_v, 3.0) * L.params.w;
    color += rim * mix(L.sky.rgb, vec3<f32>(1.0), 0.5);
    return tone_map(color * L.params.z);
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
    if (u.lighting.params.x > 1.5) {
        color = lit_matcap(color, light_sum(n, i.view_pos, i.world_pos, i.world_normal, front, i.clip.xy));
    } else if (u.lighting.params.x > 0.5) {
        color = lit_color(n, i.view_pos, i.world_pos, i.world_normal, front, i.clip.xy);
    }
    // Vertex color: tint by the paint while keeping the material's shading.
    // White (unpainted) leaves the material exactly as it is.
    // View > Show vertex color off: the plain material.
    //
    // The weight ramps smoothly over the whole distance from white, and the
    // brightening that keeps strong colors vivid grows with it. At a
    // stroke's edge the color fades from the paint to white, so a pale
    // in-between used to get the full weight and the full boost, and
    // came out brighter than both sides: a bright outline around every
    // painted area. Now the edge eases from the material into the color.
    let paint_w = smoothstep(0.0, 0.9, distance(i.paint, vec3<f32>(1.0))) * u.floor_center.z;
    if (paint_w > 0.0) {
        let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        color = mix(color, i.paint * lum * (1.0 + 0.6 * paint_w), paint_w);
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
        // Inactive pieces (View > Inactive pieces), at strength s.
        let look = piece.material.y;
        let s = piece.material.z;
        let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
        if (look > 2.5) {
            // Tint: washed with a color, keeping the shading.
            color = mix(color, piece.inactive_color.rgb * (0.35 + lum), 0.6 * s);
        } else if (look > 1.5) {
            // Darken: colors and materials kept, just darker.
            color = color * (1.0 - 0.55 * s);
        } else if (look > 0.5) {
            // Grey out: desaturated and a little darker (at s = 1, the
            // look OxyBrush always had).
            color = mix(color, vec3<f32>(lum), 0.6 * s) * (1.0 - 0.25 * s);
        }
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
