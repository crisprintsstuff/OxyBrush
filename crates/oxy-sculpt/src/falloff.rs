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

    /// [`eval`](Self::eval) with the curve's middle moved by `shift`
    /// (-1..=1): below zero the full-strength centre widens (a firmer
    /// brush), above zero it narrows (a softer one). Zero is the plain curve.
    ///
    /// The distance is reshaped as `d^k` with `k = 4^-shift`, so `d = 0`
    /// and `d = 1` stay where they are (the centre is still full strength
    /// and the edge still reaches zero) and only the shape in between moves.
    #[inline]
    pub fn eval_shifted(self, d: f32, shift: f32) -> f32 {
        if shift == 0.0 || !(0.0..1.0).contains(&d) {
            return self.eval(d);
        }
        self.eval(d.powf(shift_exponent(shift)))
    }
}

/// Exponent applied to the normalized distance for a focal shift.
#[inline]
pub fn shift_exponent(shift: f32) -> f32 {
    4f32.powf(-shift.clamp(-1.0, 1.0))
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

    #[test]
    fn focal_shift_firms_or_softens_without_moving_the_ends() {
        for f in Falloff::ALL {
            for shift in [-1.0, -0.4, 0.4, 1.0] {
                assert_eq!(f.eval_shifted(0.0, shift), 1.0, "{f:?} {shift}");
                assert_eq!(f.eval_shifted(1.0, shift), f.eval(1.0), "{f:?} {shift}");
                assert_eq!(f.eval_shifted(1.5, shift), f.eval(1.5), "{f:?} {shift}");
            }
            assert_eq!(f.eval_shifted(0.5, 0.0), f.eval(0.5));
        }
        let mid = Falloff::Smooth.eval(0.5);
        assert!(Falloff::Smooth.eval_shifted(0.5, -0.5) > mid, "negative is firmer");
        assert!(Falloff::Smooth.eval_shifted(0.5, 0.5) < mid, "positive is softer");
        // Monotonic: more shift is always softer at a given distance.
        let mut last = 2.0;
        for i in -10..=10 {
            let v = Falloff::Linear.eval_shifted(0.3, i as f32 / 10.0);
            assert!(v <= last);
            last = v;
        }
    }
}
