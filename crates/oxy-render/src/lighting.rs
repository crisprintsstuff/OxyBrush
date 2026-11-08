//! Viewport lights: an alternative to the matcap, with sun, point and spot
//! lights, optional shadows, and a simple global surface.
//!
//! This module is plain data and maths (no GPU calls): the light rig, how it
//! resolves to view-space vectors for the shader, the shadow-map matrices,
//! and how it is saved in the settings file.
//!
//! Light positions are stored relative to the scene: a position `p` means
//! `center + p * size`, where `center` and `size` are the middle and the
//! longest side of the visible pieces' bounds. A rig therefore fits any
//! model, small or large. A light that *follows the camera* is stored in the
//! camera's frame instead (x right, y up, z toward the viewer), so the model
//! turns under it as the view orbits.

use glam::{Mat4, Vec3};

/// Lights in a rig.
pub const MAX_LIGHTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightKind {
    /// Parallel light from a direction (azimuth and elevation).
    #[default]
    Sun,
    /// Light from a point, in every direction.
    Point,
    /// Light from a point, in a cone toward an aim point.
    Spot,
}

impl LightKind {
    pub const ALL: [Self; 3] = [Self::Sun, Self::Point, Self::Spot];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sun => "Sun",
            Self::Point => "Point",
            Self::Spot => "Spot",
        }
    }

    fn code(self) -> f32 {
        match self {
            Self::Sun => 0.0,
            Self::Point => 1.0,
            Self::Spot => 2.0,
        }
    }

    fn from_code(c: f32) -> Self {
        match c.round() as i32 {
            1 => Self::Point,
            2 => Self::Spot,
            _ => Self::Sun,
        }
    }
}

/// One light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub on: bool,
    pub kind: LightKind,
    /// Linear RGB, 0..1.
    pub color: [f32; 3],
    pub intensity: f32,
    /// Stored in the camera's frame, so it moves with the view.
    pub follow_camera: bool,
    /// Sun direction (toward the light), degrees: azimuth around the up
    /// axis from +Z (front), elevation above the horizon.
    pub azimuth: f32,
    pub elevation: f32,
    /// Point and spot position, relative to the scene (see the module docs).
    pub position: [f32; 3],
    /// Spot aim point, relative to the scene.
    pub aim: [f32; 3],
    /// Spot cone half-angle, degrees, and how much of it fades out (0..1).
    pub cone: f32,
    pub cone_edge: f32,
    /// Point and spot: distance falloff exponent (2 = physical inverse square,
    /// 0 = none). Distance is measured in scene sizes.
    pub falloff: f32,
    pub shadows: bool,
    /// Shadow edge softness, 0 (sharp) to 1 (very soft).
    pub softness: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            on: false,
            kind: LightKind::Sun,
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            follow_camera: false,
            azimuth: 35.0,
            elevation: 40.0,
            position: [0.8, 0.8, 0.8],
            aim: [0.0, 0.0, 0.0],
            cone: 30.0,
            cone_edge: 0.3,
            falloff: 2.0,
            shadows: false,
            softness: 0.3,
        }
    }
}

/// The whole lighting setup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// Light the viewport (instead of the matcap).
    pub enabled: bool,
    pub lights: [Light; MAX_LIGHTS],
    /// Surface color (linear RGB), roughness 0..1, specular 0..1.
    pub surface: [f32; 3],
    pub roughness: f32,
    pub specular: f32,
    /// Light from above and below everywhere (linear RGB, times `ambient`).
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    pub ambient: f32,
    /// Rim light strength (edges facing away from the viewer brighten).
    pub rim: f32,
    /// Brightness before tone mapping, in stops.
    pub exposure: f32,
    /// Shadows at all (each light also has its own switch).
    pub shadows: bool,
    /// Shadow map size per light (1024, 2048 or 4096).
    pub shadow_size: u32,
}

impl Default for Lighting {
    fn default() -> Self {
        let key = Light {
            on: true,
            kind: LightKind::Sun,
            color: [1.0, 0.93, 0.84],
            intensity: 2.6,
            follow_camera: true,
            azimuth: 35.0,
            elevation: 35.0,
            shadows: true,
            ..Light::default()
        };
        let fill = Light {
            on: true,
            kind: LightKind::Sun,
            color: [0.72, 0.82, 1.0],
            intensity: 0.7,
            follow_camera: true,
            azimuth: -60.0,
            elevation: 10.0,
            ..Light::default()
        };
        let back = Light { azimuth: 170.0, elevation: 30.0, intensity: 1.2, ..Light::default() };
        let spot = Light { kind: LightKind::Spot, position: [0.0, 1.0, 0.9], intensity: 3.0, ..Light::default() };
        Self {
            enabled: false,
            lights: [key, fill, back, spot],
            surface: [0.62, 0.42, 0.34],
            roughness: 0.55,
            specular: 0.35,
            sky: [0.55, 0.62, 0.75],
            ground: [0.28, 0.24, 0.22],
            ambient: 0.35,
            rim: 0.15,
            exposure: 0.0,
            shadows: true,
            shadow_size: 2048,
        }
    }
}

/// The camera's frame, for lights that follow it: right, up, and back
/// (toward the viewer) in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFrame {
    pub right: Vec3,
    pub up: Vec3,
    pub back: Vec3,
}

impl CameraFrame {
    pub fn new(forward: Vec3, up: Vec3) -> Self {
        let back = -forward.normalize_or(Vec3::NEG_Z);
        let right = up.cross(back).normalize_or(Vec3::X);
        Self { right, up: back.cross(right).normalize_or(Vec3::Y), back }
    }

    fn to_world(&self, v: Vec3) -> Vec3 {
        self.right * v.x + self.up * v.y + self.back * v.z
    }

    fn from_world(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.right), v.dot(self.up), v.dot(self.back))
    }
}

/// The scene's middle and size, which light positions are relative to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneFrame {
    pub center: Vec3,
    pub size: f32,
}

impl SceneFrame {
    pub fn from_bounds(min: Vec3, max: Vec3) -> Self {
        if !(min.is_finite() && max.is_finite()) || max.cmplt(min).any() {
            return Self { center: Vec3::ZERO, size: 2.0 };
        }
        Self { center: (min + max) * 0.5, size: (max - min).max_element().max(1e-3) }
    }

    /// Radius of a sphere around the scene.
    pub fn radius(&self) -> f32 {
        self.size * 0.87
    }
}

fn dir_from_angles(azimuth: f32, elevation: f32) -> Vec3 {
    let (sa, ca) = azimuth.to_radians().sin_cos();
    let (se, ce) = elevation.to_radians().sin_cos();
    Vec3::new(sa * ce, se, ca * ce)
}

/// Azimuth and elevation (degrees) of a direction.
pub fn angles_from_dir(d: Vec3) -> (f32, f32) {
    let d = d.normalize_or(Vec3::Z);
    (d.x.atan2(d.z).to_degrees(), d.y.clamp(-1.0, 1.0).asin().to_degrees())
}

impl Light {
    /// Direction toward a sun, in world space.
    pub fn sun_direction(&self, cam: &CameraFrame) -> Vec3 {
        let d = dir_from_angles(self.azimuth, self.elevation);
        if self.follow_camera { cam.to_world(d) } else { d }
    }

    fn place(&self, rel: [f32; 3], scene: &SceneFrame, cam: &CameraFrame) -> Vec3 {
        let r = Vec3::from_array(rel) * scene.size;
        scene.center + if self.follow_camera { cam.to_world(r) } else { r }
    }

    fn unplace(&self, world: Vec3, scene: &SceneFrame, cam: &CameraFrame) -> [f32; 3] {
        let r = world - scene.center;
        let r = if self.follow_camera { cam.from_world(r) } else { r };
        (r / scene.size).to_array()
    }

    /// Point or spot position in world space.
    pub fn world_position(&self, scene: &SceneFrame, cam: &CameraFrame) -> Vec3 {
        self.place(self.position, scene, cam)
    }

    pub fn set_world_position(&mut self, world: Vec3, scene: &SceneFrame, cam: &CameraFrame) {
        self.position = self.unplace(world, scene, cam);
    }

    /// Spot aim point in world space.
    pub fn world_aim(&self, scene: &SceneFrame, cam: &CameraFrame) -> Vec3 {
        self.place(self.aim, scene, cam)
    }

    pub fn set_world_aim(&mut self, world: Vec3, scene: &SceneFrame, cam: &CameraFrame) {
        self.aim = self.unplace(world, scene, cam);
    }

    /// Point a sun so it shines from `dir` (world space).
    pub fn set_sun_direction(&mut self, dir: Vec3, cam: &CameraFrame) {
        let d = if self.follow_camera { cam.from_world(dir) } else { dir };
        (self.azimuth, self.elevation) = angles_from_dir(d);
    }

    /// Where a light's marker goes: a sun sits on a sphere around the
    /// scene in its direction, a point or spot at its position.
    pub fn marker(&self, scene: &SceneFrame, cam: &CameraFrame) -> Vec3 {
        match self.kind {
            LightKind::Sun => scene.center + self.sun_direction(cam) * scene.radius() * 1.3,
            _ => self.world_position(scene, cam),
        }
    }
}

/// Approximate color of a black body at `kelvin` (1000–12000 K), linear RGB
/// normalised to its brightest channel. Uses Tanner Helland's published
/// curve fit ("How to Convert Temperature (K) to RGB", 2012) to Mitchell
/// Charity's black-body color table, which gives sRGB; good to a few
/// percent, plenty for light colors.
pub fn kelvin_to_rgb(kelvin: f32) -> [f32; 3] {
    let t = kelvin.clamp(1000.0, 12000.0) / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g = if t <= 66.0 { 99.470_8 * t.ln() - 161.119_57 } else { 288.122_16 * (t - 60.0).powf(-0.075_514_85) };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    // The fit is in sRGB; lights are linear.
    let lin = |c: f32| {
        let c = (c / 255.0).clamp(0.0, 1.0);
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let rgb = [lin(r), lin(g), lin(b)];
    let m = rgb.iter().copied().fold(1e-6, f32::max);
    rgb.map(|c| c / m)
}

// ---------------------------------------------------------------------------
// GPU packing

/// Per light, four vec4s:
/// - `a`: view-space direction toward a sun, or view-space position (xyz); w: kind (0 sun, 1 point, 2 spot)
/// - `b`: color × intensity (rgb); w: falloff exponent
/// - `c`: spot view-space direction the cone points (xyz); w: cos(outer half-angle)
/// - `d`: x: cos(inner half-angle), y: shadow layer (-1 none), z: shadow softness (texels), w: reference distance
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLight {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub c: [f32; 4],
    pub d: [f32; 4],
}

/// The lighting block appended to the viewport uniforms.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLighting {
    /// x: lit (1) or matcap (0), y: light count, z: exposure multiplier, w: rim
    pub params: [f32; 4],
    /// rgb surface color; w: roughness
    pub surface: [f32; 4],
    /// rgb sky light (× ambient); w: specular
    pub sky: [f32; 4],
    /// rgb ground light (× ambient); w: shadow map size in texels
    pub ground: [f32; 4],
    /// View-space up (xyz) for the sky/ground blend.
    pub up: [f32; 4],
    pub lights: [GpuLight; MAX_LIGHTS],
    /// World to shadow-map clip space, per shadow layer.
    pub shadow_matrices: [[[f32; 4]; 4]; MAX_LIGHTS],
}

impl GpuLighting {
    /// The matcap path: lighting off.
    pub fn off() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

/// A shadow map to draw: which light, and its world-to-clip matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowView {
    pub light: usize,
    pub matrix: Mat4,
}

/// Matrices for the lights that cast shadows (suns and spots that are on,
/// with shadows on, while shadows are on globally), at most one layer each.
pub fn shadow_views(l: &Lighting, scene: &SceneFrame, cam: &CameraFrame) -> Vec<ShadowView> {
    if !(l.enabled && l.shadows) {
        return Vec::new();
    }
    let r = scene.radius();
    let mut out = Vec::new();
    for (i, light) in l.lights.iter().enumerate() {
        if !(light.on && light.shadows && light.intensity > 0.0) {
            continue;
        }
        let matrix = match light.kind {
            LightKind::Sun => {
                // An orthographic box around the scene's bounding sphere,
                // looking along the light.
                let dir = light.sun_direction(cam);
                let eye = scene.center + dir * r * 2.0;
                let view = look_at(eye, scene.center, dir);
                ortho(-r, r, -r, r, r * 0.5, r * 3.5) * view
            }
            LightKind::Spot => {
                let pos = light.world_position(scene, cam);
                let aim = light.world_aim(scene, cam);
                let to = (aim - pos).normalize_or(-Vec3::Y);
                let view = look_at(pos, pos + to, -to);
                let dist = pos.distance(scene.center);
                let near = (dist - r).max(r * 0.02);
                let far = dist + r * 1.5;
                // A little wider than the cone, so its soft edge is covered.
                let fov = (light.cone.clamp(1.0, 85.0) * 2.0 * 1.1).to_radians().min(3.0);
                perspective(fov, near, far) * view
            }
            // Point lights would need six maps (a cube); not in this version.
            LightKind::Point => continue,
        };
        out.push(ShadowView { light: i, matrix });
    }
    out
}

/// A view matrix looking from `eye` to `target`. `away` is the direction
/// from the target back toward the eye, used to pick a stable up vector.
fn look_at(eye: Vec3, target: Vec3, away: Vec3) -> Mat4 {
    let f = (target - eye).normalize_or(-away);
    let up = if f.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols(
        s.extend(0.0).with_w(-s.dot(eye)),
        u.extend(0.0).with_w(-u.dot(eye)),
        (-f).extend(0.0).with_w(f.dot(eye)),
        glam::Vec4::W,
    )
    .transpose()
}

/// Right-handed orthographic projection to depth 0..1 (wgpu).
fn ortho(l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Mat4 {
    let rl = 1.0 / (r - l);
    let tb = 1.0 / (t - b);
    let fnr = 1.0 / (far - near);
    Mat4::from_cols(
        glam::Vec4::new(2.0 * rl, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 2.0 * tb, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, -fnr, 0.0),
        glam::Vec4::new(-(r + l) * rl, -(t + b) * tb, -near * fnr, 1.0),
    )
}

/// Right-handed perspective projection to depth 0..1 (wgpu), square aspect.
fn perspective(fov_y: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    let fnr = far / (near - far);
    Mat4::from_cols(
        glam::Vec4::new(f, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, f, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, fnr, -1.0),
        glam::Vec4::new(0.0, 0.0, near * fnr, 0.0),
    )
}

/// Pack the rig for the shader. `view` is the camera's world-to-view
/// matrix; `shadows` are the views from [`shadow_views`], in layer order.
pub fn pack(l: &Lighting, scene: &SceneFrame, cam: &CameraFrame, view: Mat4, shadows: &[ShadowView]) -> GpuLighting {
    if !l.enabled {
        return GpuLighting::off();
    }
    let mut g = GpuLighting::off();
    let to_view_dir = |d: Vec3| view.transform_vector3(d).normalize_or(Vec3::Z);
    let mut count = 0;
    for (i, light) in l.lights.iter().enumerate() {
        if !(light.on && light.intensity > 0.0) {
            continue;
        }
        let rgb = Vec3::from_array(light.color) * light.intensity;
        let layer = shadows.iter().position(|s| s.light == i).map_or(-1.0, |p| p as f32);
        let soft_texels = 0.5 + light.softness.clamp(0.0, 1.0) * 6.0;
        let (a, c) = match light.kind {
            LightKind::Sun => (to_view_dir(light.sun_direction(cam)).extend(0.0), [0.0, 0.0, -1.0, -1.0]),
            LightKind::Point => {
                (view.transform_point3(light.world_position(scene, cam)).extend(1.0), [0.0, 0.0, -1.0, -1.0])
            }
            LightKind::Spot => {
                let pos = light.world_position(scene, cam);
                let dir = to_view_dir(light.world_aim(scene, cam) - pos);
                let outer = light.cone.clamp(1.0, 89.0).to_radians();
                (view.transform_point3(pos).extend(2.0), dir.extend(outer.cos()).to_array())
            }
        };
        let outer = light.cone.clamp(1.0, 89.0).to_radians();
        let inner = outer * (1.0 - light.cone_edge.clamp(0.0, 1.0));
        g.lights[count] = GpuLight {
            a: [a.x, a.y, a.z, light.kind.code()],
            b: [rgb.x, rgb.y, rgb.z, light.falloff.clamp(0.0, 4.0)],
            c,
            d: [inner.cos(), layer, soft_texels, scene.size],
        };
        count += 1;
    }
    for (i, s) in shadows.iter().enumerate().take(MAX_LIGHTS) {
        g.shadow_matrices[i] = s.matrix.to_cols_array_2d();
    }
    let amb = l.ambient.max(0.0);
    let up = to_view_dir(Vec3::Y);
    g.params = [1.0, count as f32, 2f32.powf(l.exposure.clamp(-6.0, 6.0)), l.rim.clamp(0.0, 2.0)];
    g.surface = [l.surface[0], l.surface[1], l.surface[2], l.roughness.clamp(0.04, 1.0)];
    g.sky = [l.sky[0] * amb, l.sky[1] * amb, l.sky[2] * amb, l.specular.clamp(0.0, 1.0)];
    g.ground = [l.ground[0] * amb, l.ground[1] * amb, l.ground[2] * amb, l.shadow_size as f32];
    g.up = [up.x, up.y, up.z, 0.0];
    g
}

// ---------------------------------------------------------------------------
// Settings file

const VERSION: &str = "v1";

impl Lighting {
    /// One line for the settings file: version, globals, then each light.
    pub fn to_line(&self) -> String {
        let b = |v: bool| if v { 1.0 } else { 0.0 };
        let mut v: Vec<f32> = vec![b(self.enabled)];
        v.extend(self.surface);
        v.extend([self.roughness, self.specular]);
        v.extend(self.sky);
        v.extend(self.ground);
        v.extend([self.ambient, self.rim, self.exposure, b(self.shadows), self.shadow_size as f32]);
        for l in &self.lights {
            v.extend([b(l.on), l.kind.code()]);
            v.extend(l.color);
            v.extend([l.intensity, b(l.follow_camera), l.azimuth, l.elevation]);
            v.extend(l.position);
            v.extend(l.aim);
            v.extend([l.cone, l.cone_edge, l.falloff, b(l.shadows), l.softness]);
        }
        std::iter::once(VERSION.to_string()).chain(v.iter().map(|x| format!("{x}"))).collect::<Vec<_>>().join(" ")
    }

    /// Parse [`Self::to_line`]. Anything damaged or from another version
    /// gives `None` (the caller keeps the defaults).
    pub fn from_line(s: &str) -> Option<Self> {
        let mut it = s.split_whitespace();
        if it.next()? != VERSION {
            return None;
        }
        let v: Vec<f32> = it.map(|x| x.parse::<f32>().ok().filter(|f| f.is_finite())).collect::<Option<_>>()?;
        const GLOBALS: usize = 1 + 3 + 2 + 3 + 3 + 5;
        const PER_LIGHT: usize = 2 + 3 + 4 + 3 + 3 + 5;
        if v.len() != GLOBALS + PER_LIGHT * MAX_LIGHTS {
            return None;
        }
        let mut i = 0;
        let mut next = || {
            i += 1;
            v[i - 1]
        };
        let mut l = Lighting { enabled: next() > 0.5, ..Lighting::default() };
        l.surface = [next(), next(), next()].map(|c| c.clamp(0.0, 1.0));
        l.roughness = next().clamp(0.04, 1.0);
        l.specular = next().clamp(0.0, 1.0);
        l.sky = [next(), next(), next()].map(|c| c.clamp(0.0, 4.0));
        l.ground = [next(), next(), next()].map(|c| c.clamp(0.0, 4.0));
        l.ambient = next().clamp(0.0, 4.0);
        l.rim = next().clamp(0.0, 2.0);
        l.exposure = next().clamp(-6.0, 6.0);
        l.shadows = next() > 0.5;
        l.shadow_size = match next() as u32 {
            1024 => 1024,
            4096 => 4096,
            _ => 2048,
        };
        for light in &mut l.lights {
            light.on = next() > 0.5;
            light.kind = LightKind::from_code(next());
            light.color = [next(), next(), next()].map(|c| c.clamp(0.0, 1.0));
            light.intensity = next().clamp(0.0, 20.0);
            light.follow_camera = next() > 0.5;
            light.azimuth = next();
            light.elevation = next().clamp(-90.0, 90.0);
            light.position = [next(), next(), next()].map(|c| c.clamp(-50.0, 50.0));
            light.aim = [next(), next(), next()].map(|c| c.clamp(-50.0, 50.0));
            light.cone = next().clamp(1.0, 89.0);
            light.cone_edge = next().clamp(0.0, 1.0);
            light.falloff = next().clamp(0.0, 4.0);
            light.shadows = next() > 0.5;
            light.softness = next().clamp(0.0, 1.0);
        }
        Some(l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> SceneFrame {
        SceneFrame::from_bounds(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0))
    }

    fn front_camera() -> CameraFrame {
        CameraFrame::new(-Vec3::Z, Vec3::Y)
    }

    #[test]
    fn positions_are_relative_to_the_scene() {
        let s = scene();
        assert_eq!(s.center, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(s.size, 2.0);
        let cam = front_camera();
        let mut light = Light { kind: LightKind::Point, position: [0.5, 0.0, 0.0], ..Light::default() };
        assert_eq!(light.world_position(&s, &cam), Vec3::new(1.0, 1.0, 0.0));
        // Twice the scene: twice the offset.
        let big = SceneFrame { center: s.center, size: 4.0 };
        assert_eq!(light.world_position(&big, &cam), Vec3::new(2.0, 1.0, 0.0));
        // Setting a world position stores it back relative.
        light.set_world_position(Vec3::new(0.0, 3.0, 0.0), &s, &cam);
        assert_eq!(light.position, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn follow_camera_lights_turn_with_the_view() {
        let s = scene();
        let light = Light { follow_camera: true, azimuth: 0.0, elevation: 0.0, ..Light::default() };
        // From the front, a camera-frame sun at azimuth 0 shines from the viewer (+Z).
        assert!((light.sun_direction(&front_camera()) - Vec3::Z).length() < 1e-5);
        // Orbit to look along -X (from +X): the same light now comes from +X.
        let side = CameraFrame::new(-Vec3::X, Vec3::Y);
        assert!((light.sun_direction(&side) - Vec3::X).length() < 1e-5);
        // Round trip through set_sun_direction.
        let mut l2 = light;
        l2.set_sun_direction(Vec3::new(1.0, 1.0, 0.0).normalize(), &side);
        assert!((l2.sun_direction(&side) - Vec3::new(1.0, 1.0, 0.0).normalize()).length() < 1e-4);
        // A world-fixed light ignores the camera.
        let fixed = Light { follow_camera: false, azimuth: 90.0, elevation: 0.0, ..Light::default() };
        assert!((fixed.sun_direction(&side) - Vec3::X).length() < 1e-5);
        let _ = s;
    }

    #[test]
    fn angles_round_trip() {
        for (a, e) in [(0.0, 0.0), (45.0, 30.0), (-120.0, -20.0), (170.0, 80.0)] {
            let d = dir_from_angles(a, e);
            let (a2, e2) = angles_from_dir(d);
            assert!((a2 - a).abs() < 1e-3 && (e2 - e).abs() < 1e-3, "{a},{e} -> {a2},{e2}");
        }
    }

    #[test]
    fn sun_shadow_map_covers_the_scene() {
        let s = scene();
        let cam = front_camera();
        let l = Lighting { enabled: true, ..Lighting::default() };
        let views = shadow_views(&l, &s, &cam);
        // The default key light casts shadows; the fill doesn't.
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].light, 0);
        let m = views[0].matrix;
        // Every corner of the scene box lands inside the map, depth 0..1.
        for x in [-1.0, 1.0] {
            for y in [0.0, 2.0] {
                for z in [-1.0, 1.0] {
                    let p = m.project_point3(Vec3::new(x, y, z));
                    assert!(p.x.abs() <= 1.0 && p.y.abs() <= 1.0, "{p}");
                    assert!((0.0..=1.0).contains(&p.z), "depth {p}");
                }
            }
        }
        // Nearer the light is nearer in depth.
        let dir = l.lights[0].sun_direction(&cam);
        let near = m.project_point3(s.center + dir * 0.5).z;
        let far = m.project_point3(s.center - dir * 0.5).z;
        assert!(near < far);
    }

    #[test]
    fn spot_shadow_map_looks_at_its_aim() {
        let s = scene();
        let cam = front_camera();
        let mut l = Lighting { enabled: true, ..Lighting::default() };
        l.lights[0].shadows = false;
        l.lights[3].on = true;
        l.lights[3].shadows = true;
        let views = shadow_views(&l, &s, &cam);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].light, 3);
        let aim = l.lights[3].world_aim(&s, &cam);
        let p = views[0].matrix.project_point3(aim);
        assert!(p.x.abs() < 1e-3 && p.y.abs() < 1e-3, "the aim is in the middle of the map: {p}");
        assert!((0.0..=1.0).contains(&p.z));
        // Global switch, and point lights, cast none.
        assert!(shadow_views(&Lighting { shadows: false, ..l }, &s, &cam).is_empty());
        l.lights[3].kind = LightKind::Point;
        assert!(shadow_views(&l, &s, &cam).is_empty());
    }

    #[test]
    fn packing_skips_lights_that_are_off_and_maps_shadow_layers() {
        let s = scene();
        let cam = front_camera();
        let view = Mat4::IDENTITY;
        assert_eq!(pack(&Lighting::default(), &s, &cam, view, &[]).params[0], 0.0, "disabled: matcap");
        let l = Lighting { enabled: true, ..Lighting::default() };
        let shadows = shadow_views(&l, &s, &cam);
        let g = pack(&l, &s, &cam, view, &shadows);
        assert_eq!(g.params[1], 2.0, "key and fill are on by default");
        assert_eq!(g.lights[0].d[1], 0.0, "the key uses shadow layer 0");
        assert_eq!(g.lights[1].d[1], -1.0, "the fill casts none");
        assert_eq!(g.shadow_matrices[0], shadows[0].matrix.to_cols_array_2d());
        // Color times intensity.
        assert!((g.lights[0].b[0] - l.lights[0].color[0] * l.lights[0].intensity).abs() < 1e-6);
    }

    #[test]
    fn settings_line_round_trips_and_rejects_damage() {
        let mut l = Lighting { enabled: true, exposure: 0.5, shadow_size: 4096, ..Lighting::default() };
        l.lights[2].on = true;
        l.lights[2].kind = LightKind::Point;
        l.lights[2].position = [0.25, -0.5, 1.5];
        let line = l.to_line();
        assert_eq!(Lighting::from_line(&line), Some(l));
        assert!(Lighting::from_line("v1 1 2 3").is_none());
        assert!(Lighting::from_line(&line.replace("v1", "v9")).is_none());
        assert!(Lighting::from_line(&line.replacen(" 1 ", " nan ", 1)).is_none());
    }

    #[test]
    fn kelvin_colors_go_from_warm_to_cool() {
        let warm = kelvin_to_rgb(2700.0);
        let day = kelvin_to_rgb(6500.0);
        let cool = kelvin_to_rgb(10000.0);
        assert!(warm[0] > warm[2] * 2.0, "{warm:?}");
        assert!((day[0] - day[2]).abs() < 0.15, "about white: {day:?}");
        assert!(cool[2] > cool[0], "{cool:?}");
        assert!([warm, day, cool].iter().flatten().all(|c| (0.0..=1.0).contains(c)));
    }
}
