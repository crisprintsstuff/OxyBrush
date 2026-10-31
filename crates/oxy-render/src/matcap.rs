//! Procedurally generated matcaps. Generated at runtime so the project ships
//! no binary material assets; all output is original (CC0-compatible).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatcapPreset {
    Terracotta,
    GreyClay,
    Jade,
    Pearl,
    Bronze,
}

impl MatcapPreset {
    pub const ALL: [MatcapPreset; 5] = [Self::Terracotta, Self::GreyClay, Self::Jade, Self::Pearl, Self::Bronze];

    pub fn label(self) -> &'static str {
        match self {
            Self::Terracotta => "Terracotta",
            Self::GreyClay => "Grey clay",
            Self::Jade => "Jade",
            Self::Pearl => "Pearl",
            Self::Bronze => "Bronze",
        }
    }

    /// (base linear rgb, specular strength, shininess, rim strength)
    fn params(self) -> ([f32; 3], f32, f32, f32) {
        match self {
            Self::Terracotta => ([0.62, 0.26, 0.15], 0.25, 24.0, 0.25),
            Self::GreyClay => ([0.42, 0.42, 0.40], 0.12, 16.0, 0.2),
            Self::Jade => ([0.10, 0.38, 0.26], 0.6, 60.0, 0.35),
            Self::Pearl => ([0.75, 0.72, 0.68], 0.4, 40.0, 0.3),
            Self::Bronze => ([0.45, 0.28, 0.10], 0.9, 30.0, 0.15),
        }
    }
}

/// Square RGBA8 sRGB image of a lit sphere.
pub fn generate(preset: MatcapPreset, size: u32) -> Vec<u8> {
    let (base, spec, shin, rim) = preset.params();
    let key = normalize([-0.45, 0.55, 0.7]);
    let fill = normalize([0.6, -0.2, 0.5]);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for j in 0..size {
        for i in 0..size {
            let x = (i as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let y = 1.0 - (j as f32 + 0.5) / size as f32 * 2.0;
            let r2 = (x * x + y * y).min(1.0);
            let n = [x, y, (1.0 - r2).sqrt()];
            let kd = dot(n, key).max(0.0);
            let fd = dot(n, fill).max(0.0) * 0.25;
            // Blinn specular with the viewer along +Z.
            let h = normalize([key[0], key[1], key[2] + 1.0]);
            let ks = dot(n, h).max(0.0).powf(shin) * spec;
            let rim_t = (1.0 - n[2]).powf(3.0) * rim;
            let ambient = 0.12 + 0.08 * (y * 0.5 + 0.5);
            let mut px = [0u8; 4];
            for c in 0..3 {
                let lin = base[c] * (ambient + kd * 0.95 + fd) + ks + rim_t * 0.6;
                px[c] = (linear_to_srgb(lin) * 255.0 + 0.5) as u8;
            }
            px[3] = 255;
            out.extend_from_slice(&px);
        }
    }
    out
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

fn linear_to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}
