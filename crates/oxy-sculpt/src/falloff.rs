/// Brush falloff profile, evaluated on normalized distance `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Falloff {
    /// Smoothstep-shaped, soft edges.
    #[default]
    Smooth,
    /// Strong center, quick drop.
    Sharp,
    Linear,
    /// Flat until the edge.
    Constant,
}

impl Falloff {
    pub const ALL: [Falloff; 4] = [Falloff::Smooth, Falloff::Sharp, Falloff::Linear, Falloff::Constant];

    pub fn label(self) -> &'static str {
        match self {
            Falloff::Smooth => "Smooth",
            Falloff::Sharp => "Sharp",
            Falloff::Linear => "Linear",
            Falloff::Constant => "Constant",
        }
    }

    pub fn eval(self, d: f32) -> f32 {
        let x = (1.0 - d).clamp(0.0, 1.0);
        match self {
            Falloff::Smooth => x * x * (3.0 - 2.0 * x),
            Falloff::Sharp => x * x * x,
            Falloff::Linear => x,
            Falloff::Constant => {
                if d <= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_bounds() {
        for f in Falloff::ALL {
            assert_eq!(f.eval(0.0), 1.0, "{f:?}");
            for i in 0..=10 {
                let v = f.eval(i as f32 / 10.0);
                assert!((0.0..=1.0).contains(&v));
            }
        }
        assert_eq!(Falloff::Smooth.eval(1.0), 0.0);
    }
}
