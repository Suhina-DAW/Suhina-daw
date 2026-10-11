//! Signal-processing building blocks for Loopwerk's built-in devices (ADR 0004): gain
//! conversions, parameter smoothing, filters, crossovers and LFO waveforms.
//!
//! Nothing here depends on the engine, the model or a device: every block is plain state plus a
//! per-sample or per-block `process`, so it can be tested on its own and reused by any device.
//! Constructors and coefficient calculations may run anywhere; every `process`, `next` and
//! `reset` method is real-time safe (no allocation, locking or I/O) and never panics, whatever
//! numbers it is given.

pub mod biquad;
pub mod crossover;
pub mod dc;
pub mod gain;
pub mod lfo;
pub mod smooth;

pub use biquad::{Biquad, Coefficients};
pub use crossover::LinkwitzRiley;
pub use dc::DcBlocker;
pub use smooth::{OnePole, Smoother};

/// Highest filter frequency as a fraction of the sample rate; keeps coefficients stable just
/// below Nyquist.
pub(crate) const MAX_FREQUENCY_RATIO: f64 = 0.49;

/// `frequency` limited to what a filter at `sample_rate` can use, with non-finite input mapped
/// to 1 kHz (or the highest usable frequency, if lower).
pub(crate) fn usable_frequency(frequency: f64, sample_rate: f64) -> f64 {
    let top = usable_sample_rate(sample_rate) * MAX_FREQUENCY_RATIO;
    if frequency.is_finite() { frequency.clamp(1.0, top) } else { 1_000.0f64.min(top) }
}

/// `sample_rate`, or 48 kHz when it is not a positive finite number.
pub(crate) fn usable_sample_rate(sample_rate: f64) -> f64 {
    if sample_rate.is_finite() && sample_rate >= 1_000.0 { sample_rate } else { 48_000.0 }
}

/// Filter state below this is set to zero, so a decaying tail never reaches subnormal numbers
/// (which are slow on most CPUs).
pub(crate) const DENORMAL_FLOOR: f64 = 1e-30;

/// Filter state made safe to keep: tiny values become zero, and so does NaN, so one bad input
/// sample cannot leave a filter producing NaN forever.
#[inline]
pub(crate) fn flush(value: f64) -> f64 {
    if value.abs() >= DENORMAL_FLOOR { value } else { 0.0 }
}
