use glam::Vec3;
use std::hash::{BuildHasher, RandomState};

/// Independent maximum offsets in degrees and percentage points (RFC 0041).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintJitter {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub per_dab: bool,
}

impl PaintJitter {
    pub fn enabled(self) -> bool {
        self.hue > 0.0 || self.saturation > 0.0 || self.value > 0.0
    }

    pub fn fresh_seed() -> u64 {
        RandomState::new().hash_one(0u8)
    }

    pub(crate) fn sample(self, rgb: Vec3, seed: u64, dab: u32) -> Vec3 {
        let index = if self.per_dab { u64::from(dab) } else { 0 };
        let offset = |channel| {
            let mut bits = seed.wrapping_add(index * 3 + channel);
            bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
            bits ^= bits >> 31;
            (bits >> 40) as f32 / 8_388_608.0 - 1.0
        };

        let max = rgb.max_element();
        let chroma = max - rgb.min_element();
        let hue = if chroma == 0.0 {
            0.0
        } else if max == rgb.x {
            (rgb.y - rgb.z) / chroma
        } else if max == rgb.y {
            (rgb.z - rgb.x) / chroma + 2.0
        } else {
            (rgb.x - rgb.y) / chroma + 4.0
        };
        let saturation = if max > 0.0 { chroma / max } else { 0.0 };
        let hue = (hue + offset(0) * self.hue / 60.0).rem_euclid(6.0);
        let saturation = (saturation + offset(1) * self.saturation / 100.0).clamp(0.0, 1.0);
        let value = (max + offset(2) * self.value / 100.0).clamp(0.0, 1.0);
        let chroma = value * saturation;
        let other = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
        let rgb = match hue as u32 % 6 {
            0 => Vec3::new(chroma, other, 0.0),
            1 => Vec3::new(other, chroma, 0.0),
            2 => Vec3::new(0.0, chroma, other),
            3 => Vec3::new(0.0, other, chroma),
            4 => Vec3::new(other, 0.0, chroma),
            _ => Vec3::new(chroma, 0.0, other),
        };
        (rgb + Vec3::splat(value - chroma)).clamp(Vec3::ZERO, Vec3::ONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_modes_are_seeded_and_achromatic_colors_stay_neutral_with_hue_alone() {
        let gray = Vec3::splat(0.5);
        let hue = PaintJitter { hue: 180.0, ..Default::default() };
        assert_eq!(hue.sample(gray, 55, 0), gray);
        let jitter = PaintJitter { saturation: 60.0, value: 70.0, ..hue };
        let color = jitter.sample(gray, 55, 0);
        assert_eq!(jitter.sample(gray, 55, 19), color);
        let per_dab = PaintJitter { per_dab: true, ..jitter };
        let sequence = |seed| (0..32).map(|i| per_dab.sample(gray, seed, i)).collect::<Vec<_>>();
        let colors = sequence(55);
        assert_eq!(colors, sequence(55));
        assert_ne!(colors, sequence(56));
        assert!(colors.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(colors.iter().any(|c| c.max_element() > c.min_element()));
        assert!(colors.iter().all(|c| c.min_element() >= 0.0 && c.max_element() <= 1.0));
    }
}
