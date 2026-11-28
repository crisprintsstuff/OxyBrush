//! Parameters for the viewport's transmissive material. The versioned key
//! uses the existing per-piece material slot in an .oxb file.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassMaterial {
    pub tint: [u8; 3],
    pub roughness: f32,
    pub ior: f32,
    pub frost_seed: Option<u8>,
}

impl GlassMaterial {
    pub const CLEAR: Self = Self { tint: [255; 3], roughness: 0.04, ior: 1.5, frost_seed: None };
    pub const FROSTED: Self = Self { roughness: 0.65, frost_seed: Some(1), ..Self::CLEAR };

    pub fn label(self) -> &'static str {
        if self.frost_seed.is_some() { "Frosted Glass" } else { "Clear Glass" }
    }

    pub fn key(self) -> String {
        let [r, g, b] = self.tint;
        let seed = self.frost_seed.map_or(0, |seed| u16::from(seed) + 1);
        format!("glass:1:{r}:{g}:{b}:{}:{}:{seed}", self.roughness, self.ior)
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let mut fields = key.strip_prefix("glass:1:")?.split(':');
        let tint = [fields.next()?.parse().ok()?, fields.next()?.parse().ok()?, fields.next()?.parse().ok()?];
        let roughness = fields.next()?.parse().ok()?;
        let ior = fields.next()?.parse().ok()?;
        let seed: u16 = fields.next()?.parse().ok()?;
        if fields.next().is_some() || !(0.0..=1.0).contains(&roughness) || !(1.0..=2.5).contains(&ior) || seed > 256 {
            return None;
        }
        let frost_seed = seed.checked_sub(1).map(|v| v as u8);
        Some(Self { tint, roughness, ior, frost_seed })
    }

    pub(crate) fn uniforms(self) -> ([f32; 4], [f32; 4]) {
        let [r, g, b] = self.tint.map(|v| (f32::from(v) / 255.0).powf(2.2));
        ([r, g, b, self.ior], [self.roughness, self.frost_seed.map_or(0.0, |v| f32::from(v) + 1.0), 0.0, 0.0])
    }
}

#[cfg(test)]
mod tests {
    use super::GlassMaterial;

    #[test]
    fn project_keys_round_trip_and_reject_out_of_range_parameters() {
        for material in [GlassMaterial::CLEAR, GlassMaterial::FROSTED] {
            assert_eq!(GlassMaterial::from_key(&material.key()), Some(material));
        }
        assert!(GlassMaterial::from_key("glass:1:255:255:255:0.4:NaN:0").is_none());
        assert!(GlassMaterial::from_key("glass:1:255:255:255:0.4:1.5:257").is_none());
    }
}
