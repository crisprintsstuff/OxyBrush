//! Orbit camera for headless strokes and screenshots.
//!
//! The math matches `oxy_render::Camera` so a screen-space point means the
//! same thing in `oxybrush-cli mcp` as a pixel of the desktop viewport.
//! This copy exists so the MCP crate stays free of the renderer.

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};
use oxy_mesh::Aabb;

/// Orbit camera around a target. Fields match the viewport camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub target: Vec3,
    pub distance: f32,
    /// Radians around +Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub fov_y: f32,
    pub orthographic: bool,
    /// Viewport width and height in pixels.
    pub viewport: Vec2,
    /// Window scale. 1 in the headless virtual view.
    pub pixels_per_point: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 4.0,
            yaw: 0.0,
            pitch: 0.15,
            fov_y: 35f32.to_radians(),
            orthographic: false,
            viewport: Vec2::new(800.0, 600.0),
            pixels_per_point: 1.0,
        }
    }
}

impl View {
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

    pub fn view_matrix(&self) -> Mat4 {
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

    fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view_matrix()
    }

    /// World-space ray through a viewport pixel. `pixel` is relative to the
    /// viewport's top-left, in the same space as a screenshot.
    pub fn ray(&self, pixel: Vec2) -> (Vec3, Vec3) {
        let viewport = Vec2::new(self.viewport.x.max(1.0), self.viewport.y.max(1.0));
        let ndc = Vec2::new(pixel.x / viewport.x * 2.0 - 1.0, 1.0 - pixel.y / viewport.y * 2.0);
        let inv = self.view_projection(viewport.x / viewport.y).inverse();
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    /// World units per pixel at `at`.
    pub fn world_per_pixel(&self, at: Vec3) -> f32 {
        let height = self.viewport.y.max(1.0);
        let depth = if self.orthographic { self.distance } else { (at - self.eye()).dot(self.forward()).max(1e-4) };
        2.0 * depth * (self.fov_y * 0.5).tan() / height
    }

    /// Brush radius in world units for a slider value in logical points.
    pub fn world_radius(&self, at: Vec3, radius_pt: f32) -> f32 {
        radius_pt * self.pixels_per_point * self.world_per_pixel(at)
    }

    pub fn frame(&mut self, bounds: Aabb) {
        if bounds.is_empty() {
            return;
        }
        self.target = bounds.center();
        let radius = bounds.extent().length() * 0.5;
        self.distance = (radius / (self.fov_y * 0.5).sin()).max(1e-3) * 1.1;
    }

    pub fn snap(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Named views, matching the viewport's axis snaps.
    pub fn named(name: &str) -> Option<(f32, f32)> {
        use std::f32::consts::{FRAC_PI_2, PI};
        match name.trim().to_ascii_lowercase().as_str() {
            "front" => Some((0.0, 0.0)),
            "right" => Some((FRAC_PI_2, 0.0)),
            "back" => Some((PI, 0.0)),
            "left" => Some((-FRAC_PI_2, 0.0)),
            "top" => Some((0.0, 1.55)),
            "bottom" => Some((0.0, -1.55)),
            _ => None,
        }
    }
}

/// A short description of mirror and radial symmetry for tool replies.
pub fn symmetry_label(s: &oxy_sculpt::Symmetry) -> String {
    let mut mirrors = Vec::new();
    if s.x {
        mirrors.push("x");
    }
    if s.y {
        mirrors.push("y");
    }
    if s.z {
        mirrors.push("z");
    }
    let mirror = (!mirrors.is_empty()).then(|| format!("mirror {}", mirrors.join("+")));
    let radial = (s.radial > 1).then(|| {
        let axis = match s.radial_axis {
            oxy_sculpt::Axis::X => "x",
            oxy_sculpt::Axis::Y => "y",
            oxy_sculpt::Axis::Z => "z",
        };
        format!("radial {} around {axis}", s.radial)
    });
    match (mirror, radial) {
        (None, None) => "off".into(),
        (Some(m), None) => m,
        (None, Some(r)) => r,
        (Some(m), Some(r)) => format!("{m}, {r}"),
    }
}
