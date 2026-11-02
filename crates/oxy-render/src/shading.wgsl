// Screen-space shading passes that run after the matcap pass:
//
// 1. fs_ao: ambient obscurance from the normal/depth buffer, using the
//    estimator from McGuire et al., "The Alchemy Screen-Space Ambient
//    Obscurance Algorithm" (HPG 2011): for samples p_i on a disk around the
//    pixel, A = max(0, 1 - (2σ/s) Σ max(0, v_i·n - β) / (v_i·v_i + ε)).
// 2. fs_composite: blurs the obscurance with depth-aware weights, adds a
//    screen-space cavity term (how fast the view normal changes between
//    neighboring pixels: concave areas darken, convex edges brighten), and
//    multiplies both into the matcap color.
//
// The normal/depth buffer stores the view-space normal in xyz and the
// linear view depth in w; an all-zero texel means "no surface".

struct Post {
    // x: 1/P[0][0], y: 1/P[1][1], z: 1 if orthographic, w: unused
    proj: vec4<f32>,
    // viewport rectangle in framebuffer pixels: x, y, width, height
    viewport: vec4<f32>,
    // x: obscurance strength, y: cavity strength, z: obscurance radius (world units), w: unused
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var color_tex: texture_2d<f32>;
@group(0) @binding(2) var nd_tex: texture_2d<f32>;
@group(0) @binding(3) var ao_tex: texture_2d<f32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    return vec4<f32>(p, 0.0, 1.0);
}

fn texel(t: texture_2d<f32>, p: vec2<f32>) -> vec4<f32> {
    let lo = post.viewport.xy;
    let hi = post.viewport.xy + post.viewport.zw - vec2<f32>(1.0);
    let q = clamp(p, lo, hi);
    return textureLoad(t, vec2<i32>(q), 0);
}

fn has_surface(nd: vec4<f32>) -> bool {
    return dot(nd.xyz, nd.xyz) > 0.25;
}

// View-space position of the surface seen through framebuffer pixel `p`.
fn view_pos(p: vec2<f32>, depth: f32) -> vec3<f32> {
    let uv = (p - post.viewport.xy) / post.viewport.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    if (post.proj.z > 0.5) {
        return vec3<f32>(ndc.x * post.proj.x, ndc.y * post.proj.y, -depth);
    }
    return vec3<f32>(ndc.x * post.proj.x * depth, ndc.y * post.proj.y * depth, -depth);
}

// Interleaved gradient noise (Jimenez 2014): per-pixel sample rotation.
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

const SAMPLES: i32 = 14;
const TURNS: f32 = 7.0;
const TAU: f32 = 6.2831853;

@fragment
fn fs_ao(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let nd = texel(nd_tex, frag.xy);
    if (!has_surface(nd)) {
        return vec4<f32>(1.0);
    }
    let n = normalize(nd.xyz);
    let depth = nd.w;
    let p = view_pos(frag.xy, depth);
    let radius = post.params.z;
    // Radius in pixels at this depth.
    var per_pixel = post.proj.y * 2.0 / post.viewport.w;
    if (post.proj.z < 0.5) {
        per_pixel = per_pixel * max(depth, 1e-4);
    }
    let radius_px = clamp(radius / per_pixel, 2.0, 80.0);
    let spin = ign(frag.xy) * TAU;
    var sum = 0.0;
    for (var i = 0; i < SAMPLES; i = i + 1) {
        let a = (f32(i) + 0.5) / f32(SAMPLES);
        let angle = a * TURNS * TAU + spin;
        let q = frag.xy + vec2<f32>(cos(angle), sin(angle)) * radius_px * a;
        let sq = texel(nd_tex, q);
        if (!has_surface(sq)) {
            continue;
        }
        let v = view_pos(q, sq.w) - p;
        let vv = dot(v, v);
        // Samples far outside the radius (e.g. a piece far behind) don't count.
        let range = clamp(1.0 - vv / (4.0 * radius * radius), 0.0, 1.0);
        sum = sum + max(0.0, dot(v, n) - 0.02 * radius) / (vv + 0.01 * radius * radius) * range;
    }
    let sigma = 1.1 * radius;
    let ao = pow(max(0.0, 1.0 - 2.0 * sigma / f32(SAMPLES) * sum), 1.4);
    return vec4<f32>(ao, ao, ao, 1.0);
}

// Signed curvature estimate at scale `s` pixels: > 0 convex, < 0 concave.
fn curvature(p: vec2<f32>, nd: vec4<f32>, s: f32) -> f32 {
    let l = texel(nd_tex, p - vec2<f32>(s, 0.0));
    let r = texel(nd_tex, p + vec2<f32>(s, 0.0));
    let u = texel(nd_tex, p - vec2<f32>(0.0, s));
    let d = texel(nd_tex, p + vec2<f32>(0.0, s));
    // Ignore neighbors across silhouettes (another surface or the background).
    let limit = 0.05 * nd.w + 1e-3;
    let ok_x = has_surface(l) && has_surface(r) && abs(l.w - nd.w) < limit && abs(r.w - nd.w) < limit;
    let ok_y = has_surface(u) && has_surface(d) && abs(u.w - nd.w) < limit && abs(d.w - nd.w) < limit;
    var c = 0.0;
    if (ok_x) {
        c = c + (r.x - l.x);
    }
    // Screen y grows downward while view-space y points up.
    if (ok_y) {
        c = c + (u.y - d.y);
    }
    return c;
}

@fragment
fn fs_composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let color = texel(color_tex, frag.xy).rgb;
    let nd = texel(nd_tex, frag.xy);
    if (!has_surface(nd) || (post.params.x <= 0.0 && post.params.y <= 0.0)) {
        return vec4<f32>(color, 1.0);
    }
    // Depth-aware blur of the noisy obscurance (3x3 taps, 3 px apart,
    // matched to the per-pixel sample rotation).
    var ao_sum = 0.0;
    var w_sum = 0.0;
    let blur = select(-1, 1, post.params.x > 0.0); // no taps when obscurance is off
    for (var y = -blur; y <= blur; y = y + 1) {
        for (var x = -blur; x <= blur; x = x + 1) {
            let q = frag.xy + vec2<f32>(f32(x), f32(y)) * 3.0;
            let sq = texel(nd_tex, q);
            if (!has_surface(sq)) {
                continue;
            }
            let w = exp(-abs(sq.w - nd.w) / (0.02 * nd.w + 1e-4)) * max(dot(sq.xyz, nd.xyz), 0.0);
            ao_sum = ao_sum + texel(ao_tex, q).r * w;
            w_sum = w_sum + w;
        }
    }
    let ao = select(1.0, ao_sum / w_sum, w_sum > 1e-4);
    let ao_factor = mix(1.0, ao, post.params.x);

    // Two scales: fine pores and broader creases.
    let k = post.params.y;
    var c = 0.0;
    if (k > 0.0) {
        c = curvature(frag.xy, nd, 1.0) * 0.6 + curvature(frag.xy, nd, 3.0) * 0.4;
    }
    let valley = clamp(-c * 4.0, 0.0, 1.0);
    let ridge = clamp(c * 4.0, 0.0, 1.0);
    let shaded = color * ao_factor * (1.0 - 0.75 * k * valley) + (color * 0.5 + vec3<f32>(0.04)) * 0.6 * k * ridge;
    return vec4<f32>(shaded, 1.0);
}
