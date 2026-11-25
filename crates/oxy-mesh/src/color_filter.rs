//! Whole-piece vertex color filters: hue, saturation, value, brightness,
//! contrast, smooth, per-channel and fill. Applied to every visible vertex
//! at once and held back by the mask.
//!
//! Inspired by the public description of Blender's Color Filter tool (user
//! manual only; see `docs/CLEAN_ROOM.md`). Algorithms are standard HSV /
//! contrast / blur formulas written for OxyBrush.

use crate::Mesh;
use glam::Vec3;

/// The filters, in the order the Color Filter window lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorFilter {
    Fill,
    Hue,
    Saturation,
    Value,
    Brightness,
    Contrast,
    Smooth,
    Red,
    Green,
    Blue,
}

impl ColorFilter {
    pub const ALL: [ColorFilter; 10] = [
        ColorFilter::Fill,
        ColorFilter::Hue,
        ColorFilter::Saturation,
        ColorFilter::Value,
        ColorFilter::Brightness,
        ColorFilter::Contrast,
        ColorFilter::Smooth,
        ColorFilter::Red,
        ColorFilter::Green,
        ColorFilter::Blue,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColorFilter::Fill => "Fill",
            ColorFilter::Hue => "Hue",
            ColorFilter::Saturation => "Saturation",
            ColorFilter::Value => "Value",
            ColorFilter::Brightness => "Brightness",
            ColorFilter::Contrast => "Contrast",
            ColorFilter::Smooth => "Smooth",
            ColorFilter::Red => "Red",
            ColorFilter::Green => "Green",
            ColorFilter::Blue => "Blue",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            ColorFilter::Fill => {
                "Blend toward the current paint color (right) or toward white (left). Masked parts stay."
            }
            ColorFilter::Hue => "Rotate each color's hue. Drag right one way, left the other.",
            ColorFilter::Saturation => "Make colors richer (right) or greyer (left).",
            ColorFilter::Value => "Raise or lower each color's value (brightness in HSV).",
            ColorFilter::Brightness => "Add or subtract the same amount from every channel.",
            ColorFilter::Contrast => "Push colors away from mid-grey (right) or toward it (left).",
            ColorFilter::Smooth => "Blur colors with their neighbors (right) or sharpen (left).",
            ColorFilter::Red => "Raise or lower the red channel.",
            ColorFilter::Green => "Raise or lower the green channel.",
            ColorFilter::Blue => "Raise or lower the blue channel.",
        }
    }
}

impl Mesh {
    /// Apply a color filter to every visible vertex. `amount` is −1…1 (the
    /// bar's −100…100). Masked vertices keep their color when `use_mask` is
    /// true. `fill` is the paint color used by [`ColorFilter::Fill`].
    pub fn apply_color_filter(&mut self, kind: ColorFilter, amount: f32, fill: Vec3, use_mask: bool) {
        let amount = amount.clamp(-1.0, 1.0);
        if amount == 0.0 {
            return;
        }
        if self.color.len() != self.positions.len() {
            self.color = vec![Vec3::ONE; self.positions.len()];
        }
        match kind {
            ColorFilter::Smooth => self.filter_smooth(amount, use_mask),
            other => {
                for v in 0..self.positions.len() {
                    if self.vertex_hidden[v] {
                        continue;
                    }
                    let w = if use_mask { (1.0 - self.mask[v]).clamp(0.0, 1.0) } else { 1.0 };
                    if w == 0.0 {
                        continue;
                    }
                    let src = self.color[v];
                    let dst = filter_one(other, src, amount, fill);
                    self.color[v] = src.lerp(dst, w).clamp(Vec3::ZERO, Vec3::ONE);
                }
            }
        }
        self.mark_all_dirty();
    }

    fn filter_smooth(&mut self, amount: f32, use_mask: bool) {
        let prev = self.color.clone();
        let n = self.positions.len();
        let blur_strength = amount.abs().clamp(0.0, 1.0);
        for v in 0..n {
            if self.vertex_hidden[v] {
                continue;
            }
            let w = if use_mask { (1.0 - self.mask[v]).clamp(0.0, 1.0) } else { 1.0 };
            if w == 0.0 {
                continue;
            }
            let neighbors = self.neighbors(v as u32);
            let mut sum = Vec3::ZERO;
            let mut count = 0u32;
            for &n in &neighbors {
                let ni = n as usize;
                if !self.vertex_hidden[ni] {
                    sum += prev[ni];
                    count += 1;
                }
            }
            let blurred = if count > 0 { sum / count as f32 } else { prev[v] };
            let dst = if amount >= 0.0 {
                prev[v].lerp(blurred, blur_strength)
            } else {
                // Unsharp: push away from the blurred neighbor average.
                (prev[v] + (prev[v] - blurred) * blur_strength).clamp(Vec3::ZERO, Vec3::ONE)
            };
            self.color[v] = prev[v].lerp(dst, w).clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
}

fn filter_one(kind: ColorFilter, c: Vec3, amount: f32, fill: Vec3) -> Vec3 {
    match kind {
        ColorFilter::Fill => {
            if amount >= 0.0 {
                c.lerp(fill.clamp(Vec3::ZERO, Vec3::ONE), amount)
            } else {
                c.lerp(Vec3::ONE, -amount)
            }
        }
        ColorFilter::Hue => {
            let (h, s, v) = rgb_to_hsv(c);
            hsv_to_rgb((h + amount * 0.5).rem_euclid(1.0), s, v)
        }
        ColorFilter::Saturation => {
            let (h, s, v) = rgb_to_hsv(c);
            hsv_to_rgb(h, (s + amount).clamp(0.0, 1.0), v)
        }
        ColorFilter::Value => {
            let (h, s, v) = rgb_to_hsv(c);
            hsv_to_rgb(h, s, (v + amount).clamp(0.0, 1.0))
        }
        ColorFilter::Brightness => (c + Vec3::splat(amount)).clamp(Vec3::ZERO, Vec3::ONE),
        ColorFilter::Contrast => {
            // Pivot around mid-grey; amount −1…1 maps to a useful contrast range.
            let factor = 1.0 + amount;
            ((c - Vec3::splat(0.5)) * factor + Vec3::splat(0.5)).clamp(Vec3::ZERO, Vec3::ONE)
        }
        ColorFilter::Red => Vec3::new((c.x + amount).clamp(0.0, 1.0), c.y, c.z),
        ColorFilter::Green => Vec3::new(c.x, (c.y + amount).clamp(0.0, 1.0), c.z),
        ColorFilter::Blue => Vec3::new(c.x, c.y, (c.z + amount).clamp(0.0, 1.0)),
        ColorFilter::Smooth => c, // handled separately
    }
}

/// RGB → HSV with hue in 0…1.
fn rgb_to_hsv(c: Vec3) -> (f32, f32, f32) {
    let max = c.max_element();
    let min = c.min_element();
    let chroma = max - min;
    let h = if chroma == 0.0 {
        0.0
    } else if max == c.x {
        ((c.y - c.z) / chroma).rem_euclid(6.0) / 6.0
    } else if max == c.y {
        ((c.z - c.x) / chroma + 2.0) / 6.0
    } else {
        ((c.x - c.y) / chroma + 4.0) / 6.0
    };
    let s = if max > 0.0 { chroma / max } else { 0.0 };
    (h, s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Vec3 {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let chroma = v * s;
    let other = chroma * (1.0 - (h6.rem_euclid(2.0) - 1.0).abs());
    let rgb = match h6 as u32 % 6 {
        0 => Vec3::new(chroma, other, 0.0),
        1 => Vec3::new(other, chroma, 0.0),
        2 => Vec3::new(0.0, chroma, other),
        3 => Vec3::new(0.0, other, chroma),
        4 => Vec3::new(other, 0.0, chroma),
        _ => Vec3::new(chroma, 0.0, other),
    };
    (rgb + Vec3::splat(v - chroma)).clamp(Vec3::ZERO, Vec3::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn painted_sphere() -> Mesh {
        let mut m = primitives::icosphere(2, 1.0);
        for (i, c) in m.color.iter_mut().enumerate() {
            *c = Vec3::new(0.8, 0.2, 0.3).lerp(Vec3::new(0.1, 0.6, 0.9), (i % 7) as f32 / 6.0);
        }
        m
    }

    #[test]
    fn fill_blends_toward_paint_color_and_respects_mask() {
        let mut m = painted_sphere();
        m.mask[0] = 1.0;
        let before = m.color[0];
        let fill = Vec3::new(0.0, 1.0, 0.0);
        m.apply_color_filter(ColorFilter::Fill, 1.0, fill, true);
        assert_eq!(m.color[0], before, "fully masked vertex stays");
        assert!((m.color[1] - fill).length() < 1e-4, "unmasked fills completely at amount 1");
    }

    #[test]
    fn hue_shifts_and_zero_amount_is_noop() {
        let mut m = painted_sphere();
        let before = m.color.clone();
        m.apply_color_filter(ColorFilter::Hue, 0.0, Vec3::ONE, true);
        assert_eq!(m.color, before);
        m.apply_color_filter(ColorFilter::Hue, 0.5, Vec3::ONE, true);
        assert_ne!(m.color, before);
        // A full turn (+2 amounts of 0.5 each from original would be 360°) —
        // one half-turn should move hue, and RGB stays in range.
        assert!(m.color.iter().all(|c| c.min_element() >= 0.0 && c.max_element() <= 1.0));
    }

    #[test]
    fn saturation_zero_goes_grey_and_contrast_spreads() {
        let mut m = painted_sphere();
        m.apply_color_filter(ColorFilter::Saturation, -1.0, Vec3::ONE, false);
        for c in &m.color {
            assert!((c.x - c.y).abs() < 1e-4 && (c.y - c.z).abs() < 1e-4, "fully desaturated is grey");
        }
        let mut m = painted_sphere();
        let mid = m.color[0];
        m.apply_color_filter(ColorFilter::Contrast, 1.0, Vec3::ONE, false);
        let after = m.color[0];
        // Farther from mid-grey than before (unless already at an extreme).
        let dist = |c: Vec3| (c - Vec3::splat(0.5)).length();
        assert!(dist(after) + 1e-5 >= dist(mid));
    }

    #[test]
    fn smooth_blur_pulls_neighbors_together() {
        let mut m = primitives::icosphere(1, 1.0);
        m.color.fill(Vec3::ZERO);
        m.color[0] = Vec3::ONE;
        let neighbor = m.neighbors(0)[0] as usize;
        let before = m.color[neighbor];
        m.apply_color_filter(ColorFilter::Smooth, 1.0, Vec3::ONE, false);
        assert!(m.color[neighbor].max_element() > before.max_element(), "blur spreads white to neighbors");
        assert!(m.color[0].max_element() < 1.0, "center softens toward neighbors");
    }

    #[test]
    fn rgb_channels_and_brightness() {
        let mut m = painted_sphere();
        let before = m.color[1];
        m.apply_color_filter(ColorFilter::Red, 0.5, Vec3::ONE, false);
        assert!(m.color[1].x >= before.x);
        m.color[1] = before;
        m.apply_color_filter(ColorFilter::Brightness, -0.2, Vec3::ONE, false);
        assert!(m.color[1].x <= before.x && m.color[1].y <= before.y && m.color[1].z <= before.z);
    }

    #[test]
    fn hsv_round_trip_primary_colors() {
        for c in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::ONE, Vec3::splat(0.5), Vec3::new(0.2, 0.7, 0.4)] {
            let (h, s, v) = rgb_to_hsv(c);
            let back = hsv_to_rgb(h, s, v);
            assert!((back - c).length() < 1e-5, "{c:?} -> {back:?}");
        }
    }
}
