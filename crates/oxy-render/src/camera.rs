use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};
use oxy_mesh::Aabb;

/// Orbit camera around a target point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub distance: f32,
    /// Radians around +Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub fov_y: f32,
    pub orthographic: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self { target: Vec3::ZERO, distance: 4.0, yaw: 0.0, pitch: 0.0, fov_y: 35f32.to_radians(), orthographic: false }
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

    pub fn orbit(&mut self, delta_pixels: Vec2) {
        self.yaw -= delta_pixels.x * 0.008;
        self.pitch = (self.pitch + delta_pixels.y * 0.008).clamp(-1.55, 1.55);
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

    pub fn frame(&mut self, bounds: Aabb) {
        if bounds.is_empty() {
            return;
        }
        self.target = bounds.center();
        let radius = bounds.extent().length() * 0.5;
        self.distance = (radius / (self.fov_y * 0.5).sin()).max(1e-3) * 1.1;
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
}
