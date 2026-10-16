// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cue bus: a second stereo output for headphones, heard apart from the main mix. Tracks
//! whose cue is on (`Channel::cue`) add their sound after their own channel's volume and pan,
//! and the browser's preview voice (ADR 0011) plays into it; the whole bus is scaled by the cue
//! volume. When the cue has a pair of outputs of its own ([`Cue::separate`]) the preview is
//! heard only there; otherwise the cue plays with the main mix, so the preview is added to the
//! main output (at the cue volume) as before, and a cued track is heard once, in the main mix.
//! The buffers are allocated when the renderer is built; nothing here allocates afterwards.

use crate::chain::MAX_BLOCK;

/// The highest cue volume, linear (+12 dB).
pub const MAX_CUE_GAIN: f32 = 4.0;

pub(crate) struct Cue {
    /// The cue has an output pair of its own, so the preview is not added to the main output.
    pub(crate) separate: bool,
    /// The cue volume, linear (1.0 is 0 dB).
    pub(crate) gain: f32,
    /// The current chunk of the cue bus, interleaved stereo.
    pub(crate) bus: Vec<f32>,
    /// The current chunk of the preview voice, before it goes to the cue bus or the main output.
    pub(crate) preview: Vec<f32>,
}

impl Cue {
    pub(crate) fn new() -> Self {
        Self { separate: false, gain: 1.0, bus: vec![0.0; MAX_BLOCK * 2], preview: vec![0.0; MAX_BLOCK * 2] }
    }

    /// Sets the cue volume (clamped to 0 … [`MAX_CUE_GAIN`]; not a number is silence) and
    /// whether the cue has outputs of its own, each when given.
    pub(crate) fn set(&mut self, gain: Option<f32>, separate: Option<bool>) {
        if let Some(gain) = gain {
            self.gain = if gain.is_finite() { gain.clamp(0.0, MAX_CUE_GAIN) } else { 0.0 };
        }
        if let Some(separate) = separate {
            self.separate = separate;
        }
    }

    /// Clears the first `frames` frames of both buffers for a new chunk.
    pub(crate) fn begin(&mut self, frames: usize) {
        let end = (frames.min(MAX_BLOCK)) * 2;
        if let Some(bus) = self.bus.get_mut(..end) {
            bus.fill(0.0);
        }
        if let Some(preview) = self.preview.get_mut(..end) {
            preview.fill(0.0);
        }
    }

    /// Ends a chunk of `frames` frames: adds the preview to the cue bus, scales the bus by the
    /// cue volume, and, when the cue plays with the main mix, adds the preview to `main` at the
    /// cue volume too.
    pub(crate) fn finish(&mut self, main: &mut [f32], frames: usize) {
        let end = (frames.min(MAX_BLOCK)) * 2;
        let (Some(bus), Some(preview)) = (self.bus.get_mut(..end), self.preview.get(..end)) else { return };
        let gain = self.gain;
        for (b, p) in bus.iter_mut().zip(preview) {
            *b = (*b + p) * gain;
        }
        if !self.separate {
            for (m, p) in main.iter_mut().zip(preview) {
                *m += p * gain;
            }
        }
    }
}
