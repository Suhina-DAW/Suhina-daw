//! Waveshapers: memoryless curves that add harmonics. Run them oversampled
//! ([`Oversampler2x`](crate::Oversampler2x)) to keep the harmonics from aliasing.

/// How a shaper limits large values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// `tanh`: rounds off gradually, mostly low odd harmonics.
    Soft,
    /// Clips flat at ±1: sharper, more and higher harmonics.
    Hard,
}

impl Curve {
    /// The curve at `v`: about `v` for small values, never beyond ±1. Non-finite input gives 0.
    #[inline]
    pub fn apply(self, v: f64) -> f64 {
        if !v.is_finite() {
            return 0.0;
        }
        match self {
            Self::Soft => v.tanh(),
            Self::Hard => v.clamp(-1.0, 1.0),
        }
    }

    /// An asymmetric version that bends positive and negative halves differently, which adds
    /// even harmonics (and a DC offset the caller removes). `bias` from 0 (symmetric) to 1.
    #[inline]
    pub fn asymmetric(self, v: f64, bias: f64) -> f64 {
        self.apply(v + bias * v * v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Amplitude of harmonic `k` when `f` shapes one cycle of a sine of amplitude `a`.
    fn harmonic(f: impl Fn(f64) -> f64, a: f64, k: usize) -> f64 {
        let n = 4_096;
        let (mut re, mut im) = (0.0, 0.0);
        for i in 0..n {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            let y = f(a * t.sin());
            re += y * (k as f64 * t).cos();
            im += y * (k as f64 * t).sin();
        }
        2.0 * re.hypot(im) / n as f64
    }

    #[test]
    fn small_values_pass_and_large_ones_are_limited() {
        for curve in [Curve::Soft, Curve::Hard] {
            assert!((curve.apply(0.001) - 0.001).abs() < 1e-9);
            assert!(curve.apply(100.0) <= 1.0 && curve.apply(-100.0) >= -1.0);
            assert_eq!(curve.apply(f64::NAN), 0.0);
            assert_eq!(curve.asymmetric(f64::INFINITY, 1.0), 0.0);
        }
    }

    #[test]
    fn symmetric_curves_add_odd_harmonics_and_asymmetric_ones_even() {
        let soft = |v| Curve::Soft.apply(v);
        assert!(harmonic(soft, 2.0, 3) > 0.05);
        assert!(harmonic(soft, 2.0, 2) < 1e-9);
        let bent = |v| Curve::Soft.asymmetric(v, 0.5);
        assert!(harmonic(bent, 2.0, 2) > 0.05);
        // Hard clipping has more energy in high harmonics than soft clipping.
        let hard = |v| Curve::Hard.apply(v);
        assert!(harmonic(hard, 2.0, 9) > harmonic(soft, 2.0, 9));
    }
}
