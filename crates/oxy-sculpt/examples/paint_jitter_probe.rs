//! RFC 0041 sampling probe using the production Paint kernel on an icosphere.

use glam::Vec3;
use oxy_mesh::{Mesh, primitives::icosphere};
use oxy_sculpt::{BrushKind, BrushSettings, Stroke, StrokeSample, Symmetry};

const RADIUS: f32 = 0.10;
const DABS: usize = 22;

fn offset(dab: usize, channel: u64) -> f32 {
    // Fixed seed and independent channel inputs for a repeatable comparison.
    let mut bits = 0xc014_u64 + 3 * dab as u64 + channel;
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
    bits ^= bits >> 31;
    (bits >> 40) as f32 / 8_388_608.0 - 1.0
}

fn sample(hsv: Vec3, amount: Vec3, dab: usize) -> Vec3 {
    let delta = amount * Vec3::new(offset(dab, 0), offset(dab, 1), offset(dab, 2));
    let hue = (hsv.x + delta.x).rem_euclid(360.0) / 60.0;
    let saturation = (hsv.y + delta.y).clamp(0.0, 1.0);
    let value = (hsv.z + delta.z).clamp(0.0, 1.0);
    let chroma = value * saturation;
    let other = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let rgb = match hue as u32 {
        0 => Vec3::new(chroma, other, 0.0),
        1 => Vec3::new(other, chroma, 0.0),
        2 => Vec3::new(0.0, chroma, other),
        3 => Vec3::new(0.0, other, chroma),
        4 => Vec3::new(other, 0.0, chroma),
        _ => Vec3::new(chroma, 0.0, other),
    };
    rgb + Vec3::splat(value - chroma)
}

fn paint(base: &Mesh, spacing: f32, hsv: Vec3, amount: Vec3, per_dab: bool) -> Vec<Vec3> {
    let mut mesh = base.clone();
    let settings = BrushSettings {
        kind: BrushKind::Paint,
        radius: RADIUS,
        strength: 0.6,
        pressure_strength: false,
        pressure_radius: false,
        ..Default::default()
    };
    let mut stroke = Stroke::new(settings, Symmetry::default(), false);
    let mut centers = Vec::with_capacity(DABS);
    for dab in 0..DABS {
        let angle = (dab as f32 - (DABS - 1) as f32 * 0.5) * spacing * RADIUS;
        let center = Vec3::new(angle.sin(), 0.0, angle.cos());
        centers.push(center);
        let color = sample(hsv, amount, if per_dab { dab } else { 0 });
        stroke = stroke.with_paint_color(color);
        stroke.stamp(&mut mesh, StrokeSample { position: center, normal: center, pressure: 1.0 }, 1, &mut |_, _| {});
    }

    assert_eq!(mesh.positions, base.positions);
    centers[3..DABS - 3]
        .iter()
        .map(|&center| {
            let mut total = Vec3::ZERO;
            let mut count = 0;
            for (&point, &color) in mesh.positions.iter().zip(&mesh.color) {
                if point.distance_squared(center) <= (0.2 * RADIUS).powi(2) {
                    total += color;
                    count += 1;
                }
            }
            assert!(count > 0);
            total / count as f32
        })
        .collect()
}

fn adjacent_distance(colors: &[Vec3]) -> f32 {
    colors.windows(2).map(|pair| pair[0].distance(pair[1])).sum::<f32>() / (colors.len() - 1) as f32
}

fn main() {
    let mesh = icosphere(6, 1.0);
    let warm = Vec3::new(4.21, 0.76, 0.75);
    let gray = Vec3::new(0.0, 0.0, 0.5);
    let full = Vec3::new(25.0, 0.18, 0.18);
    let hue_only = Vec3::new(25.0, 0.0, 0.0);
    let hue_saturation = Vec3::new(25.0, 0.18, 0.0);

    println!("{} vertices; mean neighboring center-patch RGB distance", mesh.positions.len());
    for spacing in [0.2, 1.2] {
        let zero = adjacent_distance(&paint(&mesh, spacing, warm, Vec3::ZERO, true));
        let per_stroke = adjacent_distance(&paint(&mesh, spacing, warm, full, false));
        let per_dab = paint(&mesh, spacing, warm, full, true);
        assert_eq!(per_dab, paint(&mesh, spacing, warm, full, true));
        let tinted = |amount| {
            paint(&mesh, spacing, gray, amount, true)
                .iter()
                .filter(|rgb| rgb.max_element() - rgb.min_element() > 0.001)
                .count()
        };
        let hue_tinted = tinted(hue_only);
        let saturation_tinted = tinted(hue_saturation);
        println!(
            "{spacing:.1}R: zero {zero:.4}, per stroke {per_stroke:.4}, per dab {:.4}; gray tinted patches {}/{} (hue) and {}/{} (hue + saturation)",
            adjacent_distance(&per_dab),
            hue_tinted,
            per_dab.len(),
            saturation_tinted,
            per_dab.len(),
        );
        assert_eq!(hue_tinted, 0);
        assert!(saturation_tinted > 0);
    }
}
