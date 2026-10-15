//! The inference backend: what runs a model on one window (docs/spec/stem-separation.md ›
//! Backends). The chunking, resampling and channel handling around it are the crate's; a backend
//! sees one window at the model's rate and channel count, and writes each stem's share of it.

use std::fmt;

use crate::shape::ModelShape;

/// Why a backend could not run a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError(pub String);

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

/// Runs a model on one window at a time.
pub trait Backend {
    /// What the model takes and gives; checked before the first window.
    fn shape(&self) -> &ModelShape;

    /// Separates one window. `input` is `[channel][frame]`, [`ModelShape::input_len`] samples;
    /// `output` is `[stem][channel][frame]`, [`ModelShape::output_len`] samples, zeroed before
    /// each call. Windows are independent: a backend keeps no state from one to the next that
    /// changes its result, so the same input always gives the same output.
    fn run(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError>;
}
