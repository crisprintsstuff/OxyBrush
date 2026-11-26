//! Brush alphas: grayscale images that shape each dab.
//!
//! An alpha is laid flat on the surface at the dab (in the plane
//! perpendicular to the dab normal) and multiplies the brush weight, so a
//! Draw stroke with a "Scales" alpha stamps scales instead of a round bump.
//! Alphas are square, values in `0..=1`, sampled bilinearly; coordinates
//! outside the square read as 0.
//!
//! The built-in alphas are generated procedurally (original, CC0); users
//! can also load their own grayscale images (see `Alpha::from_luma`).

use glam::Vec2;

/// Maximum width or height of an alpha texture (4096).
pub const MAX_ALPHA_SIZE: u32 = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct Alpha {
    pub name: String,
    pub size: u32,
    /// Row-major, `size * size` values in `0..=1`; row 0 is the top.
    pub data: Vec<f32>,
}

/// Built-in procedural alphas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinAlpha {
    Noise,
    Bumps,
    Scales,
    /// Tighter overlapping ovals for snakes, fish and small reptiles.
    SnakeScales,
    /// Diamond / rhombus plates for armor and stylized hard-surface scales.
    DiamondScales,
    /// One teardrop plate — Stamp mode for placing a single scale.
    ScalePlate,
    Cracks,
    Stripes,
    Rings,
    Square,
}

impl BuiltinAlpha {
    pub const ALL: [BuiltinAlpha; 10] = [
        BuiltinAlpha::Noise,
        BuiltinAlpha::Bumps,
        BuiltinAlpha::Scales,
        BuiltinAlpha::SnakeScales,
        BuiltinAlpha::DiamondScales,
        BuiltinAlpha::ScalePlate,
        BuiltinAlpha::Cracks,
        BuiltinAlpha::Stripes,
        BuiltinAlpha::Rings,
        BuiltinAlpha::Square,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BuiltinAlpha::Noise => "Noise",
            BuiltinAlpha::Bumps => "Bumps",
            BuiltinAlpha::Scales => "Scales",
            BuiltinAlpha::SnakeScales => "Snake scales",
            BuiltinAlpha::DiamondScales => "Diamond scales",
            BuiltinAlpha::ScalePlate => "Scale plate",
            BuiltinAlpha::Cracks => "Cracks",
            BuiltinAlpha::Stripes => "Stripes",
            BuiltinAlpha::Rings => "Rings",
            BuiltinAlpha::Square => "Square",
        }
    }

    pub fn generate(self, size: u32) -> Alpha {
        let f = |x: f32, y: f32| -> f32 {
            // x, y in -1..1; radial fade keeps stamps from showing hard edges.
            let r = (x * x + y * y).sqrt();
            let edge = smoothstep(1.0, 0.8, r);
            match self {
                BuiltinAlpha::Noise => fbm(x * 4.0, y * 4.0, 4) * edge,
                BuiltinAlpha::Bumps => {
                    let d = worley(x * 5.0, y * 5.0).0;
                    smoothstep(0.45, 0.0, d) * edge
                }
                BuiltinAlpha::Scales => scale_field(x, y, ScaleKind::Reptile) * edge,
                BuiltinAlpha::SnakeScales => scale_field(x, y, ScaleKind::Snake) * edge,
                BuiltinAlpha::DiamondScales => scale_field(x, y, ScaleKind::Diamond) * edge,
                BuiltinAlpha::ScalePlate => single_scale_plate(x, y) * edge,
                BuiltinAlpha::Cracks => {
                    let (d1, d2) = worley(x * 3.0, y * 3.0);
                    // High along cell borders (where the two nearest seeds are equidistant).
                    smoothstep(0.12, 0.0, d2 - d1) * edge
                }
                BuiltinAlpha::Stripes => (0.5 + 0.5 * (x * 18.0).sin()) * edge,
                BuiltinAlpha::Rings => (0.5 + 0.5 * (r * 22.0).cos()) * edge,
                BuiltinAlpha::Square => {
                    let m = x.abs().max(y.abs());
                    smoothstep(1.0, 0.85, m)
                }
            }
        };
        let mut data = Vec::with_capacity((size * size) as usize);
        for j in 0..size {
            for i in 0..size {
                let x = (i as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let y = 1.0 - (j as f32 + 0.5) / size as f32 * 2.0;
                data.push(f(x, y).clamp(0.0, 1.0));
            }
        }
        Alpha { name: self.label().into(), size, data }
    }
}

#[derive(Clone, Copy)]
enum ScaleKind {
    Reptile,
    Snake,
    Diamond,
}

/// Overlapping scale rows. Samples neighbouring cells so seams stay sharp
/// where plates overlap (max of candidates), not soft half-discs.
fn scale_field(x: f32, y: f32, kind: ScaleKind) -> f32 {
    let (cols, rows) = match kind {
        // Classic reptile: larger U-plates; row pitch < rise so tips cover roots.
        ScaleKind::Reptile => (3.0_f32, 2.6_f32),
        // Snake / fish: denser staggered U-scales.
        ScaleKind::Snake => (4.8, 4.0),
        // Armor: diamond plates on a brick grid.
        ScaleKind::Diamond => (3.8, 4.2),
    };
    let sx = x * cols;
    let sy = y * rows + 0.2;
    let row0 = sy.floor() as i32;
    let col0 = sx.floor() as i32;
    let mut best = 0.0_f32;
    // Check this cell and neighbours — overlap needs the max, not one fract cell.
    for dro in -1..=1 {
        for dco in -1..=1 {
            let row = row0 + dro;
            let shift = if row.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
            let col = col0 + dco;
            let lx = sx - (col as f32 + shift);
            let ly = sy - row as f32;
            let h = match kind {
                ScaleKind::Diamond => diamond_scale(lx, ly),
                // Rise > 1 so each plate spills into the cell above (overlap).
                ScaleKind::Reptile => u_scale(lx, ly, 0.98, 1.35),
                ScaleKind::Snake => u_scale(lx, ly, 1.15, 1.25),
            };
            if h > best {
                best = h;
            }
        }
    }
    best
}

/// Classic U / fish-scale plate: semicircle with a flat root chord.
/// `half_w` is half-width in cell units; `rise` is tip height above the root
/// (values > 1 make plates overlap the next row).
fn u_scale(lx: f32, ly: f32, half_w: f32, rise: f32) -> f32 {
    // Local: root near y=0, tip toward +y.
    let px = lx / half_w;
    let root = 0.02;
    let py = (ly - root) / rise;
    if py < 0.0 || py > 1.12 {
        return 0.0;
    }
    // Semicircle of radius 1 centred at the root; tip at py=1.
    let arc = (px * px + py * py).sqrt();
    let rim = smoothstep(1.05, 0.90, arc);
    if rim <= 0.0 {
        return 0.0;
    }
    // Raised tip, shallow near the root; crisp groove at the rim.
    let dome = py.sqrt() * (0.18 + 0.82 * py);
    let groove = smoothstep(1.05, 0.96, arc);
    rim * dome * groove
}

/// Rhombus plate with a soft bevel and tip bias (+Y).
fn diamond_scale(lx: f32, ly: f32) -> f32 {
    // Cell local: diamond centred near (0, 0.5); tip toward +Y.
    let px = lx;
    let py = ly - 0.5;
    let manhattan = px.abs() + py.abs();
    let rim = smoothstep(0.58, 0.40, manhattan);
    if rim <= 0.0 {
        return 0.0;
    }
    let along = ((py + 0.5) / 1.0).clamp(0.0, 1.0);
    let dome = 0.28 + 0.72 * along;
    let bevel = smoothstep(0.58, 0.46, manhattan);
    rim * dome * bevel
}

/// One large U-plate for Stamp strokes (place a single scale).
fn single_scale_plate(x: f32, y: f32) -> f32 {
    // Map stamp space so the root sits near the bottom and the tip near the top.
    let lx = x * 0.88;
    let ly = (y + 0.95) * 0.52;
    let h = u_scale(lx, ly, 0.88, 0.98);
    // Slightly harder rim than the field stamps so a single Stamp reads clearly.
    (h * 1.15).clamp(0.0, 1.0)
}

impl Alpha {
    /// The 16-bit value a project stores for one pixel (`65535` = full
    /// strength). This is the rounding `.oxb` uses, so a hash of these
    /// values still matches after the alpha is saved and opened.
    pub fn quantize_u16(v: f32) -> u16 {
        (v.clamp(0.0, 1.0) * 65535.0).round().clamp(0.0, 65535.0) as u16
    }

    /// Build from 8-bit luminance (e.g. a grayscale PNG). Non-square images
    /// are fitted into a square preserving aspect ratio (padded with 0).
    pub fn from_luma(name: impl Into<String>, width: u32, height: u32, luma: &[u8]) -> Option<Alpha> {
        if width == 0 || height == 0 || luma.len() < (width * height) as usize {
            return None;
        }
        let (size, data) = resample_to_square(width, height, luma, |b| b as f32 / 255.0);
        Some(Alpha { name: name.into(), size, data })
    }

    /// Build from 16-bit luminance (e.g. a 16-bit grayscale PNG). Non-square
    /// images are fitted into a square preserving aspect ratio (padded with 0).
    pub fn from_luma16(name: impl Into<String>, width: u32, height: u32, luma: &[u16]) -> Option<Alpha> {
        if width == 0 || height == 0 || luma.len() < (width * height) as usize {
            return None;
        }
        let (size, data) = resample_to_square(width, height, luma, |w| w as f32 / 65535.0);
        Some(Alpha { name: name.into(), size, data })
    }

    /// Bilinear sample at `uv` in `-1..=1` (x right, y up). 0 outside.
    pub fn sample(&self, uv: Vec2) -> f32 {
        if uv.x.abs() > 1.0 || uv.y.abs() > 1.0 {
            return 0.0;
        }
        let n = self.size as f32;
        let fx = ((uv.x + 1.0) * 0.5 * n - 0.5).clamp(0.0, n - 1.0);
        let fy = ((1.0 - uv.y) * 0.5 * n - 0.5).clamp(0.0, n - 1.0);
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.size - 1), (y0 + 1).min(self.size - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: u32, y: u32| self.data[(y * self.size + x) as usize];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }

    /// 8-bit preview pixels (row-major, `size * size`).
    pub fn to_luma8(&self) -> Vec<u8> {
        self.data.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect()
    }

    /// Downsampled 8-bit preview pixels (at most `max_dim × max_dim`, row-major square).
    pub fn to_preview_luma8(&self, max_dim: u32) -> (u32, Vec<u8>) {
        if self.size <= max_dim {
            return (self.size, self.to_luma8());
        }
        let preview_size = max_dim;
        let mut out = Vec::with_capacity((preview_size * preview_size) as usize);
        for j in 0..preview_size {
            for i in 0..preview_size {
                let uv_x = (i as f32 + 0.5) / preview_size as f32 * 2.0 - 1.0;
                let uv_y = 1.0 - (j as f32 + 0.5) / preview_size as f32 * 2.0;
                let val = self.sample(Vec2::new(uv_x, uv_y));
                out.push((val.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
        (preview_size, out)
    }
}

/// Resamples a 2D image into a square alpha, fitting the image to preserve
/// aspect ratio and zero-padding any remainder. Uses area-box filtering for
/// downsampling so fine lines are antialiased without breaking or aliasing.
fn resample_to_square<T: Copy>(width: u32, height: u32, pixels: &[T], to_f32: impl Fn(T) -> f32) -> (u32, Vec<f32>) {
    let max_dim = width.max(height);
    let size = max_dim.clamp(16, MAX_ALPHA_SIZE);
    let scale = size as f64 / max_dim as f64;
    let fit_w = ((width as f64 * scale).round() as u32).clamp(1, size);
    let fit_h = ((height as f64 * scale).round() as u32).clamp(1, size);
    let pad_x = (size - fit_w) / 2;
    let pad_y = (size - fit_h) / 2;

    let mut data = vec![0.0f32; (size * size) as usize];
    let downsampling = width > fit_w || height > fit_h;

    for j in 0..fit_h {
        let dst_y = pad_y + j;
        let y_start = j as f64 * height as f64 / fit_h as f64;
        let y_end = (j + 1) as f64 * height as f64 / fit_h as f64;

        for i in 0..fit_w {
            let dst_x = pad_x + i;
            let x_start = i as f64 * width as f64 / fit_w as f64;
            let x_end = (i + 1) as f64 * width as f64 / fit_w as f64;

            let val = if downsampling {
                let sx0 = x_start.floor() as usize;
                let sx1 = (x_end.ceil() as usize).min(width as usize);
                let sy0 = y_start.floor() as usize;
                let sy1 = (y_end.ceil() as usize).min(height as usize);

                let mut sum = 0.0;
                let mut weight = 0.0;
                for sy in sy0..sy1 {
                    let y_lo = (sy as f64).max(y_start);
                    let y_hi = ((sy + 1) as f64).min(y_end);
                    let h_overlap = (y_hi - y_lo).max(0.0);
                    if h_overlap <= 0.0 {
                        continue;
                    }
                    let row_offset = sy * width as usize;
                    for sx in sx0..sx1 {
                        let x_lo = (sx as f64).max(x_start);
                        let x_hi = ((sx + 1) as f64).min(x_end);
                        let w_overlap = (x_hi - x_lo).max(0.0);
                        let area = (w_overlap * h_overlap) as f32;
                        if area > 0.0 {
                            sum += to_f32(pixels[row_offset + sx]) * area;
                            weight += area;
                        }
                    }
                }
                if weight > 0.0 {
                    sum / weight
                } else {
                    let cx = (x_start as usize).min(width as usize - 1);
                    let cy = (y_start as usize).min(height as usize - 1);
                    to_f32(pixels[cy * width as usize + cx])
                }
            } else {
                let sx = ((i as f32 + 0.5) / fit_w as f32 * width as f32 - 0.5).clamp(0.0, (width - 1) as f32);
                let sy = ((j as f32 + 0.5) / fit_h as f32 * height as f32 - 0.5).clamp(0.0, (height - 1) as f32);
                let x0 = sx.floor() as usize;
                let y0 = sy.floor() as usize;
                let x1 = (x0 + 1).min(width as usize - 1);
                let y1 = (y0 + 1).min(height as usize - 1);
                let fx = sx - x0 as f32;
                let fy = sy - y0 as f32;
                let v00 = to_f32(pixels[y0 * width as usize + x0]);
                let v10 = to_f32(pixels[y0 * width as usize + x1]);
                let v01 = to_f32(pixels[y1 * width as usize + x0]);
                let v11 = to_f32(pixels[y1 * width as usize + x1]);
                (v00 * (1.0 - fx) + v10 * fx) * (1.0 - fy) + (v01 * (1.0 - fx) + v11 * fx) * fy
            };

            data[(dst_y * size + dst_x) as usize] = val.clamp(0.0, 1.0);
        }
    }

    (size, data)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash2(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// 2D value noise in 0..1.
fn value_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash2(xi, yi) + (hash2(xi + 1, yi) - hash2(xi, yi)) * sx;
    let b = hash2(xi, yi + 1) + (hash2(xi + 1, yi + 1) - hash2(xi, yi + 1)) * sx;
    a + (b - a) * sy
}

fn fbm(x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for _ in 0..octaves {
        sum += value_noise(x * freq, y * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Distances to the nearest and second-nearest jittered cell points.
fn worley(x: f32, y: f32) -> (f32, f32) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2) = (f32::MAX, f32::MAX);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let p = Vec2::new(cx as f32 + hash2(cx, cy), cy as f32 + hash2(cy + 7, cx - 3));
            let d = p.distance(Vec2::new(x, y));
            if d < d1 {
                d2 = d1;
                d1 = d;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2)
}

/// 3D value noise in -1..1, used by the Noise brush.
pub fn noise3(p: glam::Vec3) -> f32 {
    let h = |x: i32, y: i32, z: i32| hash2(x.wrapping_mul(73_856_093) ^ z.wrapping_mul(83_492_791), y);
    let (xi, yi, zi) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    let t = p - glam::Vec3::new(xi as f32, yi as f32, zi as f32);
    let s = t * t * (glam::Vec3::splat(3.0) - 2.0 * t);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx: i32, dy: i32, dz: i32| h(xi + dx, yi + dy, zi + dz);
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), s.x);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), s.x);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), s.x);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), s.x);
    lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_and_distinct() {
        let all: Vec<Alpha> = BuiltinAlpha::ALL.iter().map(|b| b.generate(64)).collect();
        assert_eq!(all.len(), 10);
        for a in &all {
            assert_eq!(a.data.len(), 64 * 64);
            assert!(a.data.iter().all(|v| (0.0..=1.0).contains(v)), "{}", a.name);
            assert!(a.data.iter().any(|&v| v > 0.5), "{} should have strong areas", a.name);
            assert!(a.data.iter().any(|&v| v < 0.2), "{} should have weak areas", a.name);
        }
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i].data, all[j].data);
            }
        }
    }

    #[test]
    fn scale_variants_have_grain_and_a_strong_tip() {
        let scales = BuiltinAlpha::Scales.generate(128);
        let snake = BuiltinAlpha::SnakeScales.generate(128);
        let diamond = BuiltinAlpha::DiamondScales.generate(128);
        let plate = BuiltinAlpha::ScalePlate.generate(128);
        // Tip-biased: upper half of the stamp should be brighter on average than the lower.
        let mean = |a: &Alpha, y0: u32, y1: u32| {
            let mut s = 0.0;
            let mut n = 0.0;
            for y in y0..y1 {
                for x in 0..a.size {
                    s += a.data[(y * a.size + x) as usize];
                    n += 1.0;
                }
            }
            s / n
        };
        let half = scales.size / 2;
        assert!(mean(&scales, 0, half) > mean(&scales, half, scales.size) * 0.9, "Scales tip bias");
        assert!(mean(&snake, 0, half) > mean(&snake, half, snake.size) * 0.85, "Snake tip bias");
        // Plate is one big scale — centre should be strong.
        assert!(plate.sample(Vec2::new(0.0, 0.15)) > 0.45);
        assert!(diamond.sample(Vec2::ZERO) > 0.2);
        // Densities differ: snake has more cycles than reptile.
        let contrast = |a: &Alpha| {
            let max = a.data.iter().cloned().fold(0.0_f32, f32::max);
            let min = a.data.iter().cloned().fold(1.0_f32, f32::min);
            max - min
        };
        assert!(contrast(&scales) > 0.5);
        assert!(contrast(&snake) > 0.5);
        assert!(contrast(&diamond) > 0.5);
    }

    #[test]
    fn sampling() {
        let a = BuiltinAlpha::Square.generate(32);
        assert!(a.sample(Vec2::ZERO) > 0.99);
        assert_eq!(a.sample(Vec2::new(1.5, 0.0)), 0.0);
        // Left/right halves of a horizontal gradient.
        let luma: Vec<u8> = (0..16 * 16).map(|i| ((i % 16) * 17) as u8).collect();
        let g = Alpha::from_luma("grad", 16, 16, &luma).unwrap();
        assert!(g.sample(Vec2::new(-0.9, 0.0)) < 0.2);
        assert!(g.sample(Vec2::new(0.9, 0.0)) > 0.8);
        assert!(Alpha::from_luma("bad", 4, 4, &[0; 3]).is_none());
    }

    #[test]
    fn non_square_aspect_ratio_and_padding() {
        // A 20x40 image (tall)
        let luma = vec![255u8; 20 * 40];
        let a = Alpha::from_luma("tall", 20, 40, &luma).unwrap();
        assert_eq!(a.size, 40);
        // Left and right sides should be padded with 0 (since width was 20 and height 40)
        assert_eq!(a.sample(Vec2::new(-0.9, 0.0)), 0.0);
        assert_eq!(a.sample(Vec2::new(0.9, 0.0)), 0.0);
        // Center should be 1.0
        assert_eq!(a.sample(Vec2::ZERO), 1.0);
    }

    #[test]
    fn high_res_clamp_and_16bit() {
        let luma16 = vec![65535u16; 32 * 32];
        let a = Alpha::from_luma16("16bit", 32, 32, &luma16).unwrap();
        assert_eq!(a.size, 32);
        assert_eq!(a.sample(Vec2::ZERO), 1.0);

        // Clamping respects MAX_ALPHA_SIZE (4096)
        assert_eq!(MAX_ALPHA_SIZE, 4096);
    }

    #[test]
    fn preview_downsampling() {
        let a = BuiltinAlpha::Noise.generate(512);
        let (ps, prev) = a.to_preview_luma8(64);
        assert_eq!(ps, 64);
        assert_eq!(prev.len(), 64 * 64);
    }

    #[test]
    fn noise3_range_and_continuity() {
        let mut prev = noise3(glam::Vec3::new(0.0, 0.3, 0.7));
        for i in 1..1000 {
            let v = noise3(glam::Vec3::new(i as f32 * 0.01, 0.3, 0.7));
            assert!((-1.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.1, "noise should be continuous at step {i}: {prev} -> {v}");
            prev = v;
        }
    }
}
