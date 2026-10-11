//! Band splitting.

use crate::biquad::{BUTTERWORTH_Q, Biquad, Coefficients};

/// A 4th-order (24 dB/octave) Linkwitz-Riley crossover for one channel: two Butterworth
/// sections in series on each side. Both bands are -6 dB at the crossover frequency and their
/// sum has a flat magnitude response (it is an all-pass), so splitting a signal, processing the
/// bands and adding them back changes only what the processing changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkwitzRiley {
    low: [Biquad; 2],
    high: [Biquad; 2],
}

impl LinkwitzRiley {
    pub fn new(sample_rate: f64, frequency: f64) -> Self {
        let mut crossover = Self { low: [Biquad::default(); 2], high: [Biquad::default(); 2] };
        crossover.set_frequency(sample_rate, frequency);
        crossover
    }

    /// Moves the crossover frequency; the filter state is kept.
    pub fn set_frequency(&mut self, sample_rate: f64, frequency: f64) {
        let low = Coefficients::lowpass(sample_rate, frequency, BUTTERWORTH_Q);
        let high = Coefficients::highpass(sample_rate, frequency, BUTTERWORTH_Q);
        self.low.iter_mut().for_each(|s| s.set(low));
        self.high.iter_mut().for_each(|s| s.set(high));
    }

    /// Splits one sample into its (low, high) bands.
    #[inline]
    pub fn split(&mut self, x: f64) -> (f64, f64) {
        let [l1, l2] = &mut self.low;
        let [h1, h2] = &mut self.high;
        (l2.process(l1.process(x)), h2.process(h1.process(x)))
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
}
