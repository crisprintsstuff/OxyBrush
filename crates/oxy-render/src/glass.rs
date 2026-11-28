//! Parameters for the viewport's transmissive material. The versioned key
//! uses the existing per-piece material slot in an .oxb file.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassMaterial {
    pub tint: [u8; 3],
    pub roughness: f32,
    pub ior: f32,
    pub frost_seed: Option<u8>,
    /// Thickness-based tint strength per scene unit. Zero uses uniform tint.
    pub absorption: f32,
    /// Trace a second refraction through the back of a closed piece.
    pub exit_refraction: bool,
}

impl GlassMaterial {
    pub const CLEAR: Self =
        Self { tint: [255; 3], roughness: 0.04, ior: 1.5, frost_seed: None, absorption: 0.0, exit_refraction: false };
    pub const FROSTED: Self = Self { roughness: 0.65, frost_seed: Some(1), ..Self::CLEAR };

    pub fn label(self) -> &'static str {
        if self.frost_seed.is_some() { "Frosted Glass" } else { "Clear Glass" }
    }

    pub fn key(self) -> String {
        let [r, g, b] = self.tint;
        let seed = self.frost_seed.map_or(0, |seed| u16::from(seed) + 1);
        if self.exit_refraction {
            format!("glass:3:{r}:{g}:{b}:{}:{}:{seed}:{}", self.roughness, self.ior, self.absorption)
        } else if self.absorption > 0.0 {
            format!("glass:2:{r}:{g}:{b}:{}:{}:{seed}:{}", self.roughness, self.ior, self.absorption)
        } else {
            format!("glass:1:{r}:{g}:{b}:{}:{}:{seed}", self.roughness, self.ior)
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let (version, body) = key.strip_prefix("glass:")?.split_once(':')?;
        let mut fields = body.split(':');
        let tint = [fields.next()?.parse().ok()?, fields.next()?.parse().ok()?, fields.next()?.parse().ok()?];
        let roughness = fields.next()?.parse().ok()?;
        let ior = fields.next()?.parse().ok()?;
        let seed: u16 = fields.next()?.parse().ok()?;
        let absorption = match version {
            "1" => 0.0,
            "2" | "3" => fields.next()?.parse().ok()?,
            _ => return None,
        };
        let exit_refraction = version == "3";
        if fields.next().is_some()
            || !(0.0..=1.0).contains(&roughness)
            || !(1.0..=2.5).contains(&ior)
            || seed > 256
            || !(0.0..=4.0).contains(&absorption)
        {
            return None;
        }
        let frost_seed = seed.checked_sub(1).map(|v| v as u8);
        Some(Self { tint, roughness, ior, frost_seed, absorption, exit_refraction })
    }

    pub(crate) fn uses_exit_refraction(self) -> bool {
        self.exit_refraction && self.ior > 1.0
    }

    pub(crate) fn absorbs(self) -> bool {
        self.absorption > 0.0 && self.tint != [255; 3]
    }

    pub(crate) fn uniforms(self, instanced: bool) -> ([f32; 4], [f32; 4]) {
        let [r, g, b] = self.tint.map(|v| (f32::from(v) / 255.0).powf(2.2));
        let frost = self.frost_seed.map_or(0.0, |v| f32::from(v) + 1.0);
        let absorption = if !instanced && self.absorbs() { self.absorption } else { 0.0 };
        let exit = f32::from(u8::from(!instanced && self.uses_exit_refraction()));
        ([r, g, b, self.ior], [self.roughness, frost, absorption, exit])
    }
}

#[cfg(test)]
mod tests {
    use super::GlassMaterial;

    #[test]
    fn project_keys_round_trip_and_reject_out_of_range_parameters() {
        let colored = GlassMaterial { tint: [130, 200, 255], absorption: 1.2, ..GlassMaterial::FROSTED };
        let exit = GlassMaterial { exit_refraction: true, ..colored };
        let untinted_exit = GlassMaterial { exit_refraction: true, ..GlassMaterial::CLEAR };
        for material in [GlassMaterial::CLEAR, GlassMaterial::FROSTED, colored, exit, untinted_exit] {
            assert_eq!(GlassMaterial::from_key(&material.key()), Some(material));
        }
        assert_eq!(GlassMaterial::from_key("glass:1:255:255:255:0.04:1.5:0"), Some(GlassMaterial::CLEAR));
        assert_eq!(GlassMaterial::from_key("glass:2:130:200:255:0.65:1.5:2:1.2"), Some(colored));
        assert!(GlassMaterial::from_key("glass:3:255:255:255:0.4:1.5:0").is_none());
        assert!(GlassMaterial::from_key("glass:4:255:255:255:0.4:1.5:0:0").is_none());
        assert!(GlassMaterial::from_key("glass:2:255:255:255:0.4:1.5:0").is_none());
        assert!(GlassMaterial::from_key("glass:2:255:255:255:0.4:1.5:0:inf").is_none());
        assert!(GlassMaterial::from_key("glass:2:255:255:255:0.4:1.5:0:-0.01").is_none());
        assert!(GlassMaterial::from_key("glass:2:255:255:255:0.4:1.5:0:4.01").is_none());
        assert!(GlassMaterial::from_key("glass:1:255:255:255:0.4:NaN:0").is_none());
        assert!(GlassMaterial::from_key("glass:1:255:255:255:0.4:1.5:257").is_none());
    }
}
