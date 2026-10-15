//! The test backend (docs/spec/stem-separation.md › The test backend): a deterministic band
//! splitter with no weights, so the whole pipeline runs in `cargo test` without a third-party
//! model. It is no separator of instruments; it is a stand-in whose output is known.
//!
//! Each crossover is a second-order Butterworth low-pass run over the window from silence. The
//! first stem is the lowest low-pass, each next one the difference between two neighbouring
//! low-passes, and the last one the input minus the highest low-pass, so the stems of every
//! window add up to its input (to rounding), whatever the filters do.

use crate::backend::{Backend, BackendError};
use crate::shape::{ModelShape, Weighting};

/// A band splitter with `crossovers.len() + 1` stems.
#[derive(Debug, Clone)]
pub struct BandSplitter {
    shape: ModelShape,
    /// Biquad coefficients per crossover, lowest first: b0, b1, b2, a1, a2.
    filters: Vec<[f64; 5]>,
    lows: Vec<f32>,
}

impl BandSplitter {
    /// A splitter at `sample_rate` with the given crossovers in hertz, ascending and below the
    /// Nyquist frequency. The shape it reports is checked with [`ModelShape::validate`] by the
    /// pipeline, as any backend's is.
    pub fn new(
        sample_rate: u32,
        channels: usize,
        window: usize,
        hop: usize,
        weighting: Weighting,
        crossovers: &[f64],
    ) -> Result<Self, BackendError> {
        let nyquist = f64::from(sample_rate) / 2.0;
        let mut last = 0.0;
        for &hz in crossovers {
            if !hz.is_finite() || hz <= last || hz >= nyquist {
                return Err(BackendError(format!("crossover {hz} Hz is not ascending between 0 and {nyquist} Hz")));
            }
            last = hz;
        }
        let shape = ModelShape { sample_rate, channels, window, hop, weighting, stems: crossovers.len() + 1 };
        let filters = crossovers.iter().map(|&hz| low_pass(hz, f64::from(sample_rate))).collect();
        Ok(BandSplitter { shape, filters, lows: Vec::new() })
    }
}

/// Second-order Butterworth low-pass coefficients (bilinear transform, Q = 1/√2).
fn low_pass(hz: f64, rate: f64) -> [f64; 5] {
    let w = std::f64::consts::TAU * hz / rate;
    let alpha = w.sin() / std::f64::consts::SQRT_2;
    let cos = w.cos();
    let a0 = 1.0 + alpha;
    let b1 = (1.0 - cos) / a0;
    [b1 / 2.0, b1, b1 / 2.0, -2.0 * cos / a0, (1.0 - alpha) / a0]
}

/// `input` through the biquad `c`, from silence, into `out`.
fn filter(c: &[f64; 5], input: &[f32], out: &mut [f32]) {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (o, &x) in out.iter_mut().zip(input) {
        let x = f64::from(x);
        let y = c[0] * x + c[1] * x1 + c[2] * x2 - c[3] * y1 - c[4] * y2;
        (x2, x1, y2, y1) = (x1, x, y1, y);
        *o = y as f32;
    }
}

impl Backend for BandSplitter {
    fn shape(&self) -> &ModelShape {
        &self.shape
    }

    fn run(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError> {
        let window = self.shape.window;
        let channels = self.shape.channels;
        if input.len() != self.shape.input_len() || output.len() != self.shape.output_len() {
            return Err(BackendError("the window's buffers do not match the shape".into()));
        }
        // One low-passed copy per crossover, `[crossover][frame]`, for one channel at a time.
        self.lows.resize(self.filters.len() * window, 0.0);
        let stem_len = channels * window;
        for (channel, x) in input.chunks_exact(window).enumerate() {
            for (c, low) in self.filters.iter().zip(self.lows.chunks_exact_mut(window)) {
                filter(c, x, low);
            }
            for stem in 0..self.shape.stems {
                let at = stem * stem_len + channel * window;
                let Some(out) = output.get_mut(at..at + window) else { continue };
                let below = stem.checked_sub(1).and_then(|s| self.lows.get(s * window..(s + 1) * window));
                let upper =
                    if stem < self.filters.len() { self.lows.get(stem * window..(stem + 1) * window) } else { Some(x) };
                let Some(upper) = upper else { continue };
                for (i, o) in out.iter_mut().enumerate() {
                    let high = upper.get(i).copied().unwrap_or(0.0);
                    let low = below.and_then(|b| b.get(i)).copied().unwrap_or(0.0);
                    *o = high - low;
                }
            }
        }
        Ok(())
    }
}
