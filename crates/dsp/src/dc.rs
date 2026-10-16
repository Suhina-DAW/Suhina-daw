// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! DC removal.

use crate::{flush, usable_sample_rate};

/// Corner frequency of [`DcBlocker`], in Hz: low enough to keep the lowest musical notes.
pub const DC_CORNER_HZ: f64 = 5.0;

/// A first-order high-pass that removes DC offset: `y[n] = x[n] - x[n-1] + r * y[n-1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DcBlocker {
    r: f64,
    x1: f64,
    y1: f64,
}

impl DcBlocker {
    pub fn new(sample_rate: f64) -> Self {
        let r = (-std::f64::consts::TAU * DC_CORNER_HZ / usable_sample_rate(sample_rate)).exp();
        Self { r, x1: 0.0, y1: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_dc_and_keeps_audio() {
        let sr = 48_000.0;
        let mut dc = DcBlocker::new(sr);
        let mut last = 0.0;
        for _ in 0..48_000 {
            last = dc.process(0.5);
        }
        assert!(last.abs() < 1e-3, "{last}");

        let mut dc = DcBlocker::new(sr);
        let mut peak = 0.0f64;
        for n in 0..48_000 {
            let x = 0.3 + (std::f64::consts::TAU * 100.0 * n as f64 / sr).sin();
            let y = dc.process(x);
            if n > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        assert!((peak - 1.0).abs() < 0.01, "{peak}");
    }
}
