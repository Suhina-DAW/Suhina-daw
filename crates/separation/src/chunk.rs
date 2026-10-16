// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cutting a signal into the model's windows and adding the windows' outputs back together
//! (docs/spec/stem-separation.md › Chunking).
//!
//! Windows start every `hop` frames from frame 0. The last one starts early enough to cover the
//! final frame and is padded with zeros past the end (the padded tail). Each window's output is
//! multiplied by the weighting and added into one accumulator per stem and channel; the sum of
//! the weights at each frame, the same for every stem and channel, is kept once and divided out
//! at the end.

use crate::shape::ModelShape;

/// Where the windows over a signal of `frames` frames start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub frames: usize,
    pub window: usize,
    pub hop: usize,
    /// Windows needed to cover every frame.
    pub count: usize,
}

impl Plan {
    /// The windows over `frames` frames for a validated shape; at least one, even for a signal
    /// shorter than a window.
    pub fn new(frames: usize, shape: &ModelShape) -> Self {
        let (window, hop) = (shape.window, shape.hop.max(1));
        let count = if frames <= window { 1 } else { 1 + (frames - window).div_ceil(hop) };
        Plan { frames, window, hop, count }
    }

    /// The first frame of window `index`.
    pub fn start(&self, index: usize) -> usize {
        index.saturating_mul(self.hop)
    }

    /// Frames of window `index` that hold the signal; the rest of it is the zero padding.
    pub fn filled(&self, index: usize) -> usize {
        self.frames.saturating_sub(self.start(index)).min(self.window)
    }
}

/// Copies window `index` of each channel into `input` (`[channel][frame]`, `window` frames per
/// channel), padding past the signal's end with zeros.
pub fn fill_window(plan: &Plan, index: usize, channels: &[&[f32]], input: &mut [f32]) {
    let start = plan.start(index);
    let filled = plan.filled(index);
    for (channel, dest) in channels.iter().zip(input.chunks_exact_mut(plan.window)) {
        let source = channel.get(start..start + filled).unwrap_or(&[]);
        let (head, tail) = dest.split_at_mut(source.len());
        head.copy_from_slice(source);
        tail.fill(0.0);
    }
}

/// The weighted sum of every window's output, and the sum of the weights at each frame.
pub struct OverlapAdd {
    plan: Plan,
    weights: Vec<f32>,
    /// `[stem][channel][frame]`, `frames` frames each.
    sums: Vec<f32>,
    /// The weights added at each frame.
    divider: Vec<f32>,
    lanes: usize,
}

impl OverlapAdd {
    /// An empty accumulator for `lanes` (stems × channels) signals. The caller has checked that
    /// `lanes × frames` samples may be allocated.
    pub fn new(plan: Plan, shape: &ModelShape, lanes: usize) -> Self {
        OverlapAdd {
            plan,
            weights: shape.weighting.weights(plan.window),
            sums: vec![0.0; lanes.saturating_mul(plan.frames)],
            divider: vec![0.0; plan.frames],
            lanes,
        }
    }

    /// Adds window `index`'s output (`[lane][frame]`, `window` frames per lane), weighted. The
    /// padded part past the signal's end is dropped.
    pub fn add(&mut self, index: usize, output: &[f32]) {
        let start = self.plan.start(index);
        let filled = self.plan.filled(index);
        let weights = self.weights.get(..filled).unwrap_or(&[]);
        let frames = self.plan.frames;
        for (lane, window) in output.chunks_exact(self.plan.window).take(self.lanes).enumerate() {
            let at = lane * frames + start;
            let Some(dest) = self.sums.get_mut(at..at + filled) else { continue };
            for ((d, s), w) in dest.iter_mut().zip(window).zip(weights) {
                *d += s * w;
            }
        }
        if let Some(dest) = self.divider.get_mut(start..start + filled) {
            for (d, w) in dest.iter_mut().zip(weights) {
                *d += w;
            }
        }
    }

    /// Divides the weights out and returns the lanes, `[lane][frame]`. Every frame is covered by
    /// a window with a weight above zero, so no division is by zero.
    pub fn finish(mut self) -> Vec<f32> {
        let frames = self.plan.frames.max(1);
        for lane in self.sums.chunks_exact_mut(frames) {
            for (s, d) in lane.iter_mut().zip(&self.divider) {
                if *d > 0.0 {
                    *s /= d;
                }
            }
        }
        self.sums
    }
}
