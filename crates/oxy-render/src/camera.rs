use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};
use oxy_mesh::Aabb;

/// Height of a 35mm full-frame gate. Focal length is the 35mm-equivalent
/// lens that produces `fov_y` vertically on this gate:
/// `fov_y = 2 * atan(12 mm / focal_mm)`.
pub const FULL_FRAME_HEIGHT_MM: f32 = 24.0;

/// Shortest and longest lens the viewport offers, in millimeters.
pub const FOCAL_MM_MIN: f32 = 15.0;
pub const FOCAL_MM_MAX: f32 = 200.0;

/// Vertical field of view the viewport has always shipped with.
pub const DEFAULT_FOV_Y: f32 = 35f32.to_radians();

/// Common 35mm-equivalent lenses. The default lens (35° vertical, about
/// 38 mm) is not one of these, so Reset can put that view back exactly.
pub const FOCAL_PRESETS_MM: [f32; 5] = [18.0, 35.0, 50.0, 85.0, 135.0];

/// Lens length for `fov_y`, clamped to [`FOCAL_MM_MIN`]..=[`FOCAL_MM_MAX`].
pub fn focal_mm_from_fov_y(fov_y: f32) -> f32 {
    let t = (fov_y * 0.5).tan();
    if !t.is_finite() || t <= 1e-8 {
        return FOCAL_MM_MAX;
    }
    ((FULL_FRAME_HEIGHT_MM * 0.5) / t).clamp(FOCAL_MM_MIN, FOCAL_MM_MAX)
}

/// The default lens, rounded the same way the settings file stores it, so
/// a save and load does not nudge the historic 35° view.
pub fn default_focal_mm() -> f32 {
    let exact = focal_mm_from_fov_y(DEFAULT_FOV_Y);
    (exact * 10_000.0).round() / 10_000.0
}

/// Finite values clamp into the lens range; anything else is the default.
pub fn clamp_focal_mm(mm: f32) -> f32 {
    if mm.is_finite() { mm.clamp(FOCAL_MM_MIN, FOCAL_MM_MAX) } else { default_focal_mm() }
}

/// Snap to 0.01 mm. A value that rounds to the default lens stays on the
/// exact default, so the field of view remains 35°.
pub fn quantize_focal_mm(mm: f32) -> f32 {
    let q = (clamp_focal_mm(mm) * 100.0).round() / 100.0;
    if (q - default_focal_mm()).abs() < 0.005 { default_focal_mm() } else { clamp_focal_mm(q) }
}

/// Vertical field of view, in radians, for a 35mm-equivalent focal length.
pub fn fov_y_from_focal_mm(mm: f32) -> f32 {
    let mm = quantize_focal_mm(mm);
    if (mm - default_focal_mm()).abs() < 1e-4 {
        DEFAULT_FOV_Y
    } else {
        2.0 * ((FULL_FRAME_HEIGHT_MM * 0.5) / mm).atan()
    }
}

/// Orbit camera around a target point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub distance: f32,
    /// Radians around +Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    /// Vertical field of view, radians. Perspective only; orthographic
    /// size is kept stable when this changes (see `set_fov_y_keeping_framing`).
    pub fov_y: f32,
    pub orthographic: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self { target: Vec3::ZERO, distance: 4.0, yaw: 0.0, pitch: 0.0, fov_y: DEFAULT_FOV_Y, orthographic: false }
    }
}

impl Camera {
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        -Vec3::new(sy * cp, sp, cy * cp)
    }

    pub fn eye(&self) -> Vec3 {
        self.target - self.forward() * self.distance
    }

    pub fn up(&self) -> Vec3 {
        let right = self.forward().cross(Vec3::Y).normalize_or(Vec3::X);
        right.cross(self.forward()).normalize()
    }

    pub fn view(&self) -> Mat4 {
        look_at_mat4(self.eye(), self.target, self.up())
    }

    fn near_far(&self) -> (f32, f32) {
        (self.distance * 0.01, self.distance * 100.0)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let (near, far) = self.near_far();
        if self.orthographic {
            let h = self.distance * (self.fov_y * 0.5).tan();
            let w = h * aspect;
            directx::orthographic(-w, w, -h, h, -far, far)
        } else {
            directx::perspective(self.fov_y, aspect, near, far)
        }
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// World-space ray through a pixel. `pixel` is relative to the viewport's top-left.
    pub fn ray(&self, pixel: Vec2, viewport: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(pixel.x / viewport.x * 2.0 - 1.0, 1.0 - pixel.y / viewport.y * 2.0);
        let inv = self.view_projection(viewport.x / viewport.y).inverse();
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    /// Project a world point to viewport pixels; None if behind the camera.
    pub fn project(&self, p: Vec3, viewport: Vec2) -> Option<Vec2> {
        let clip = self.view_projection(viewport.x / viewport.y) * p.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new((ndc.x + 1.0) * 0.5 * viewport.x, (1.0 - ndc.y) * 0.5 * viewport.y))
    }

    /// World units per pixel at the given point (used for screen-space brush radius).
    pub fn world_per_pixel(&self, at: Vec3, viewport_height: f32) -> f32 {
        let depth = if self.orthographic { self.distance } else { (at - self.eye()).dot(self.forward()).max(1e-4) };
        2.0 * depth * (self.fov_y * 0.5).tan() / viewport_height.max(1.0)
    }

    /// 35mm-equivalent focal length for the current vertical field of view.
    pub fn focal_length_mm(&self) -> f32 {
        focal_mm_from_fov_y(self.fov_y)
    }

    /// Horizontal and vertical field of view in degrees. `aspect` is
    /// viewport width / height; horizontal follows the window.
    pub fn fov_degrees(&self, aspect: f32) -> (f32, f32) {
        let aspect = if aspect.is_finite() && aspect > 1e-3 { aspect } else { 1.0 };
        let vertical = self.fov_y.to_degrees();
        let horizontal = 2.0 * (aspect * (self.fov_y * 0.5).tan()).atan().to_degrees();
        (horizontal, vertical)
    }

    /// Change the vertical field of view without resizing the subject at
    /// the orbit target. `distance` scales with `tan(fov/2)`, which is also
    /// the orthographic half-height, so an orthographic view stays put and
    /// a later switch back to perspective keeps the same framing.
    pub fn set_fov_y_keeping_framing(&mut self, fov_y: f32) {
        let min_fov = fov_y_from_focal_mm(FOCAL_MM_MAX);
        let max_fov = fov_y_from_focal_mm(FOCAL_MM_MIN);
        let new_fov = if fov_y.is_finite() { fov_y.clamp(min_fov, max_fov) } else { DEFAULT_FOV_Y };
        if (new_fov - self.fov_y).abs() < 1e-7 {
            self.fov_y = new_fov;
            return;
        }
        let old_tan = (self.fov_y * 0.5).tan();
        let new_tan = (new_fov * 0.5).tan();
        if old_tan.is_finite() && new_tan.is_finite() && old_tan > 1e-8 && new_tan > 1e-8 {
            self.distance = (self.distance * old_tan / new_tan).clamp(1e-3, 1e5);
        }
        self.fov_y = new_fov;
    }

    /// Set the 35mm-equivalent focal length, keeping the framing at the target.
    pub fn set_focal_length_mm(&mut self, mm: f32) {
        self.set_fov_y_keeping_framing(fov_y_from_focal_mm(mm));
    }

    pub fn orbit(&mut self, delta_pixels: Vec2) {
        self.yaw -= delta_pixels.x * 0.008;
        self.pitch = (self.pitch + delta_pixels.y * 0.008).clamp(-1.55, 1.55);
    }

    /// Orbit in `step`-radian increments. `free` carries the unsnapped
    /// angles between calls (start it at the current yaw and pitch), so
    /// slow drags still reach the next step.
    pub fn orbit_snapped(&mut self, free: &mut (f32, f32), delta_pixels: Vec2, step: f32) {
        free.0 -= delta_pixels.x * 0.008;
        free.1 = (free.1 + delta_pixels.y * 0.008).clamp(-1.55, 1.55);
        self.yaw = (free.0 / step).round() * step;
        // Straight down/up is just short of ±90° (the camera keeps an up vector).
        self.pitch = ((free.1 / step).round() * step).clamp(-1.55, 1.55);
    }

    pub fn pan(&mut self, delta_pixels: Vec2, viewport_height: f32) {
        let s = self.world_per_pixel(self.target, viewport_height);
        let right = self.forward().cross(self.up());
        self.target += (-right * delta_pixels.x + self.up() * delta_pixels.y) * s;
    }

    /// Positive `steps` zooms in.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.9f32.powf(steps)).clamp(1e-3, 1e5);
    }

    /// Fit `bounds` in view.
    ///
    /// Perspective backs the camera up until the bounding sphere sits inside
    /// the current field of view. Orthographic uses the fit of the default
    /// lens, so the focal length does not change how large the model is;
    /// `distance` is then set so switching back to perspective keeps that
    /// framing.
    pub fn frame(&mut self, bounds: Aabb) {
        if bounds.is_empty() {
            return;
        }
        self.target = bounds.center();
        let radius = bounds.extent().length() * 0.5;
        if self.orthographic {
            // `radius / cos(default/2) * 1.1` is the half-height `frame`
            // has always produced at the default lens (`distance * tan`).
            let half = (radius / (DEFAULT_FOV_Y * 0.5).cos()).max(1e-6) * 1.1;
            let tan = (self.fov_y * 0.5).tan().max(1e-6);
            self.distance = (half / tan).clamp(1e-3, 1e5);
        } else {
            self.distance = (radius / (self.fov_y * 0.5).sin()).max(1e-3) * 1.1;
        }
    }

    /// Snap to an axis view (e.g. front, side, top).
    pub fn snap(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapped_orbit_lands_on_steps_and_accumulates() {
        let mut c = Camera { yaw: 0.0, pitch: 0.0, ..Default::default() };
        let mut free = (c.yaw, c.pitch);
        let step = std::f32::consts::FRAC_PI_4;
        // Small moves accumulate until the next step is closer.
        for _ in 0..20 {
            c.orbit_snapped(&mut free, Vec2::new(-5.0, 0.0), step);
        }
        assert!((c.yaw - step).abs() < 1e-6, "{}", c.yaw);
        c.orbit_snapped(&mut free, Vec2::new(0.0, 1000.0), step);
        assert!((c.pitch - 1.55).abs() < 1e-6, "pitch stops just short of straight down");
    }

    #[test]
    fn ray_through_center_hits_target() {
        let c = Camera { yaw: 0.7, pitch: 0.3, ..Default::default() };
        let (o, d) = c.ray(Vec2::new(400.0, 300.0), Vec2::new(800.0, 600.0));
        let closest = o + d * (c.target - o).dot(d);
        assert!(closest.distance(c.target) < 1e-3);
    }

    #[test]
    fn project_inverts_ray() {
        let c = Camera::default();
        let vp = Vec2::new(800.0, 600.0);
        let px = Vec2::new(123.0, 456.0);
        let (o, d) = c.ray(px, vp);
        let back = c.project(o + d * 3.0, vp).unwrap();
        assert!(back.distance(px) < 0.05, "{back}");
    }

    fn half_height(c: &Camera) -> f32 {
        c.distance * (c.fov_y * 0.5).tan()
    }

    #[test]
    fn default_lens_is_the_historic_field_of_view() {
        let c = Camera::default();
        assert!((c.fov_y - DEFAULT_FOV_Y).abs() < 1e-6);
        assert!((c.fov_y.to_degrees() - 35.0).abs() < 1e-4);
        let mm = default_focal_mm();
        assert!((15.0..=200.0).contains(&mm), "{mm}");
        assert!((fov_y_from_focal_mm(mm) - DEFAULT_FOV_Y).abs() < 1e-6);
        // The settings file writes four decimal places and reads them back.
        let text = format!("{mm:.4}");
        let parsed: f32 = text.parse().unwrap();
        assert!((parsed - mm).abs() < 1e-6, "{text} -> {parsed} vs {mm}");
        assert!((quantize_focal_mm(mm) - mm).abs() < 1e-6);
        // Typing the displayed 0.01 mm value (38.06) keeps 35° exactly.
        assert!((fov_y_from_focal_mm((mm * 100.0).round() / 100.0) - DEFAULT_FOV_Y).abs() < 1e-6);
    }

    #[test]
    fn focal_length_keeps_target_size_and_changes_perspective() {
        let mut c = Camera { yaw: 0.4, pitch: 0.25, ..Default::default() };
        let vp = Vec2::new(1280.0, 720.0);
        let on_plane = c.target + c.up() * 0.45;
        let before = c.project(on_plane, vp).unwrap();
        let center = c.project(c.target, vp).unwrap();
        let span = half_height(&c);
        c.set_focal_length_mm(18.0);
        let after = c.project(on_plane, vp).unwrap();
        assert!(before.distance(after) < 0.35, "target plane stays put: {before} -> {after}");
        assert!((half_height(&c) - span).abs() / span < 1e-4);
        assert!((c.focal_length_mm() - 18.0).abs() < 0.02, "{}", c.focal_length_mm());

        // A point closer than the target, and off the view axis, grows on a
        // wide lens and shrinks on a long one: the camera dollies, so
        // perspective actually changes. (A point on the axis always
        // projects to the centre.)
        let front = c.target - c.forward() * 1.0 + c.up() * 0.25;
        let wide = c.project(front, vp).unwrap().distance(c.project(c.target, vp).unwrap());
        c.set_focal_length_mm(135.0);
        let tele = c.project(front, vp).unwrap().distance(c.project(c.target, vp).unwrap());
        assert!(wide > tele * 1.5, "wide {wide} px, tele {tele} px, center was {center}");
        // Zoom still changes framing after the lens does.
        let framed = half_height(&c);
        c.zoom(1.0);
        assert!(half_height(&c) < framed * 0.95);
    }

    #[test]
    fn orthographic_view_ignores_focal_length() {
        let mut wide = Camera { yaw: 0.5, pitch: -0.2, orthographic: true, ..Default::default() };
        wide.set_focal_length_mm(18.0);
        let mut tele = wide;
        tele.set_focal_length_mm(200.0);
        // X/Y scale of the orthographic matrix is the framing. Z depends on
        // `distance` (the clip range), which the dolly changes on purpose.
        let aspect = 1.7;
        let pw = wide.projection(aspect).to_cols_array();
        let pt = tele.projection(aspect).to_cols_array();
        for i in [0, 5] {
            let (a, b) = (pw[i], pt[i]);
            let scale = a.abs().max(b.abs()).max(1.0);
            assert!((a - b).abs() / scale < 1e-4, "projection [{i}] {a} vs {b}");
        }
        let vp = Vec2::new(800.0, 600.0);
        let p = wide.target + wide.up() * 0.3 + wide.forward().cross(wide.up()).normalize() * 0.2;
        let a = wide.project(p, vp).unwrap();
        let b = tele.project(p, vp).unwrap();
        assert!(a.distance(b) < 0.05, "{a} vs {b}");
        assert!((wide.world_per_pixel(wide.target, 600.0) - tele.world_per_pixel(tele.target, 600.0)).abs() < 1e-5);

        let bounds = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        wide.frame(bounds);
        tele.frame(bounds);
        assert!((half_height(&wide) - half_height(&tele)).abs() / half_height(&wide) < 1e-4);
        // At the default lens, orthographic frame matches the old distance formula.
        let mut classic = Camera { orthographic: true, ..Default::default() };
        classic.frame(bounds);
        let radius = bounds.extent().length() * 0.5;
        let old = (radius / (DEFAULT_FOV_Y * 0.5).sin()) * 1.1;
        assert!((classic.distance - old).abs() / old < 1e-4, "{} vs {old}", classic.distance);
    }

    #[test]
    fn focal_length_clamps_to_the_offered_range() {
        let mut c = Camera::default();
        c.set_focal_length_mm(1.0);
        assert!((c.focal_length_mm() - FOCAL_MM_MIN).abs() < 0.02);
        c.set_focal_length_mm(10_000.0);
        assert!((c.focal_length_mm() - FOCAL_MM_MAX).abs() < 0.05);
        c.set_focal_length_mm(f32::NAN);
        assert!((c.fov_y - DEFAULT_FOV_Y).abs() < 1e-5);
        assert!((quantize_focal_mm(f32::INFINITY) - default_focal_mm()).abs() < 1e-6);
    }
}
