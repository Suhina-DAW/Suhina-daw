// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Band splitting.

use crate::biquad::{BUTTERWORTH_Q, Biquad, Coefficients};

/// Steepness of a [`LinkwitzRiley`] crossover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slope {
    /// 4th order: two 2nd-order Butterworth sections per band.
    Db24,
    /// 8th order: a 4th-order Butterworth filter, twice, per band.
    Db48,
}

impl Slope {
    /// Q of each section, in order.
    fn qs(self) -> &'static [f64] {
        // A 4th-order Butterworth filter is two sections with Q = 1 / (2 cos(π/8)) and
        // 1 / (2 cos(3π/8)).
        const Q1: f64 = 0.541_196_100_146_197;
        const Q2: f64 = 1.306_562_964_876_376_7;
        match self {
            Self::Db24 => &[BUTTERWORTH_Q, BUTTERWORTH_Q],
            Self::Db48 => &[Q1, Q2, Q1, Q2],
        }
    }
}

const MAX_SECTIONS: usize = 4;

/// A Linkwitz-Riley crossover for one channel. Both bands are -6 dB at the crossover frequency
/// and their sum has a flat magnitude response (it is an all-pass), so splitting a signal,
/// processing the bands and adding them back changes only what the processing changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkwitzRiley {
    low: [Biquad; MAX_SECTIONS],
    high: [Biquad; MAX_SECTIONS],
    slope: Slope,
    sample_rate: f64,
    frequency: f64,
}

impl LinkwitzRiley {
    /// A 24 dB/octave crossover.
    pub fn new(sample_rate: f64, frequency: f64) -> Self {
        Self::with_slope(sample_rate, frequency, Slope::Db24)
    }

    pub fn with_slope(sample_rate: f64, frequency: f64, slope: Slope) -> Self {
        let mut crossover = Self {
            low: [Biquad::default(); MAX_SECTIONS],
            high: [Biquad::default(); MAX_SECTIONS],
            slope,
            sample_rate,
            frequency,
        };
        crossover.set_frequency(sample_rate, frequency);
        crossover
    }

    /// Moves the crossover frequency; the filter state is kept.
    pub fn set_frequency(&mut self, sample_rate: f64, frequency: f64) {
        self.sample_rate = sample_rate;
        self.frequency = frequency;
        for (i, &q) in self.slope.qs().iter().enumerate() {
            if let (Some(low), Some(high)) = (self.low.get_mut(i), self.high.get_mut(i)) {
                low.set(Coefficients::lowpass(sample_rate, frequency, q));
                high.set(Coefficients::highpass(sample_rate, frequency, q));
            }
        }
    }

    /// Changes the slope. The sections that a steeper slope adds start from silence.
    pub fn set_slope(&mut self, slope: Slope) {
        if slope == self.slope {
            return;
        }
        let before = self.slope.qs().len();
        self.slope = slope;
        self.set_frequency(self.sample_rate, self.frequency);
        for section in self.low.iter_mut().skip(before).chain(self.high.iter_mut().skip(before)) {
            section.reset();
        }
    }

    pub fn slope(&self) -> Slope {
        self.slope
    }

    /// Splits one sample into its (low, high) bands.
    #[inline]
    pub fn split(&mut self, x: f64) -> (f64, f64) {
        let sections = self.slope.qs().len();
        let mut low = x;
        let mut high = x;
        for (l, h) in self.low.iter_mut().zip(&mut self.high).take(sections) {
            low = l.process(low);
            high = h.process(high);
        }
        (low, high)
    }

    /// The sum of both bands: an all-pass with the crossover's phase response. A band split
    /// at another frequency goes through this so all bands line up in phase when summed.
    #[inline]
    pub fn allpass(&mut self, x: f64) -> f64 {
        let (low, high) = self.split(x);
        low + high
    }

    pub fn reset(&mut self) {
        self.low.iter_mut().chain(&mut self.high).for_each(Biquad::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biquad::tests::{SR, measured_gain};

    #[test]
    fn bands_are_half_level_at_the_crossover() {
        let mut low = LinkwitzRiley::new(SR, 200.0);
        let mut high = LinkwitzRiley::new(SR, 200.0);
        assert!((measured_gain(200.0, |x| low.split(x).0) - 0.5).abs() < 0.01);
        assert!((measured_gain(200.0, |x| high.split(x).1) - 0.5).abs() < 0.01);
        let mut low = LinkwitzRiley::new(SR, 200.0);
        assert!(measured_gain(3_200.0, |x| low.split(x).0) < 0.0003);
    }

    #[test]
    fn the_bands_add_up_to_a_flat_response() {
        for f in [30.0, 120.0, 200.0, 350.0, 1_000.0, 8_000.0] {
            let mut crossover = LinkwitzRiley::new(SR, 200.0);
            let gain = measured_gain(f, |x| {
                let (low, high) = crossover.split(x);
                low + high
            });
            assert!((gain - 1.0).abs() < 0.01, "{f} Hz: {gain}");
        }
    }

    #[test]
    fn the_48_db_slope_is_steeper_and_still_flat_when_summed() {
        let mut low = LinkwitzRiley::with_slope(SR, 200.0, Slope::Db48);
        assert!((measured_gain(200.0, |x| low.split(x).0) - 0.5).abs() < 0.01);
        // Two octaves above the corner the steeper slope is far lower.
        let mut steep = LinkwitzRiley::with_slope(SR, 200.0, Slope::Db48);
        let mut gentle = LinkwitzRiley::new(SR, 200.0);
        let (s, g) = (measured_gain(800.0, |x| steep.split(x).0), measured_gain(800.0, |x| gentle.split(x).0));
        assert!(s < g / 10.0, "{s} {g}");
        for f in [40.0, 200.0, 900.0, 6_000.0] {
            let mut crossover = LinkwitzRiley::with_slope(SR, 200.0, Slope::Db48);
            let gain = measured_gain(f, |x| crossover.allpass(x));
            assert!((gain - 1.0).abs() < 0.01, "{f} Hz: {gain}");
        }
    }

    #[test]
    fn changing_the_slope_keeps_running() {
        let mut crossover = LinkwitzRiley::new(SR, 500.0);
        for n in 0..1_000 {
            crossover.split((n as f64 * 0.1).sin());
        }
        crossover.set_slope(Slope::Db48);
        assert_eq!(crossover.slope(), Slope::Db48);
        assert!(crossover.low[2..].iter().chain(&crossover.high[2..]).all(|s| *s == Biquad::new(s.coefficients())));
        let gain = measured_gain(5_000.0, |x| crossover.split(x).1);
        assert!((gain - 1.0).abs() < 0.01, "{gain}");
        crossover.set_slope(Slope::Db24);
        let gain = measured_gain(500.0, |x| crossover.allpass(x));
        assert!((gain - 1.0).abs() < 0.01, "{gain}");
    }
}
