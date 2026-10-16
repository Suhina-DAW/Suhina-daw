// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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

    /// An asymmetric version: the negative half limits earlier, at `-1 / (1 + bias)`, which
    /// adds even harmonics (and a DC offset the caller removes). Both halves start with slope
    /// 1 and the curve always rises. `bias` from 0 (symmetric) to 1; other values are limited
    /// to that range.
    #[inline]
    pub fn asymmetric(self, v: f64, bias: f64) -> f64 {
        let k = 1.0 + if bias.is_finite() { bias.clamp(0.0, 1.0) } else { 0.0 };
        if v >= 0.0 { self.apply(v) } else { self.apply(k * v) / k }
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
            // The asymmetric curve always rises, and its negative half stops at -1 / (1 + bias).
            let values: Vec<f64> = (-400..=400).map(|i| curve.asymmetric(f64::from(i) * 0.05, 0.5)).collect();
            assert!(values.windows(2).all(|w| w[1] >= w[0]), "{curve:?}");
            assert!((curve.asymmetric(-100.0, 0.5) + 1.0 / 1.5).abs() < 1e-9);
            assert!((curve.asymmetric(0.001, 0.5) - 0.001).abs() < 1e-6);
            assert!((curve.asymmetric(-0.001, 0.5) + 0.001).abs() < 1e-6);
        }
    }

    #[test]
    fn symmetric_curves_add_odd_harmonics_and_asymmetric_ones_even() {
        let soft = |v| Curve::Soft.apply(v);
        assert!(harmonic(soft, 2.0, 3) > 0.05);
        assert!(harmonic(soft, 2.0, 2) < 1e-9);
        let bent = |v| Curve::Soft.asymmetric(v, 0.5);
        assert!(harmonic(bent, 2.0, 2) > 0.05);
        let hard_bent = |v| Curve::Hard.asymmetric(v, 1.0);
        assert!(harmonic(hard_bent, 2.0, 2) > 0.05);
        // Hard clipping has more energy in high harmonics than soft clipping.
        let hard = |v| Curve::Hard.apply(v);
        assert!(harmonic(hard, 2.0, 9) > harmonic(soft, 2.0, 9));
    }
}
