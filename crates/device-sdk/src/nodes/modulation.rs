// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub const OSCILLATOR: NodeInfo = NodeInfo {
    kind: "oscillator",
    name: "Oscillator",
    inputs: &[PortInfo { name: "frequency", default: Some(1.0), unit: "Hz" }],
    outputs: &["out", "left", "right"],
    description: "Free-running phase in cycles, frequency limited to +/-0.49*sample rate. Starts at phase zero on reset. Waveforms are not band-limited.",
    configuration: " shape: Waveform",
    example: "{\"kind\":\"oscillator\",\"shape\":\"sine\"}",
    rust_variant: "Oscillator",
};

pub const SYNCED_LFO: NodeInfo = NodeInfo {
    kind: "synced_lfo",
    name: "Synced LFO",
    inputs: &[PortInfo { name: "rate", default: Some(1.0), unit: "cycles/beat" }],
    outputs: &["out", "left", "right"],
    description: "Waveform at transport beat * rate; follows song position, including stopped transport. No phase state.",
    configuration: " shape: Waveform",
    example: "{\"kind\":\"synced_lfo\",\"shape\":\"sine\"}",
    rust_variant: "SyncedLfo",
};

pub const NOISE: NodeInfo = NodeInfo {
    kind: "noise",
    name: "Noise",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Deterministic xorshift32 white noise -1..1, broadcast to both channels; seed zero becomes one. Reset restores seed.",
    configuration: " seed: u32",
    example: "{\"kind\":\"noise\",\"seed\":1}",
    rust_variant: "Noise",
};

pub const SMOOTH: NodeInfo = NodeInfo {
    kind: "smooth",
    name: "Smooth",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "time", default: Some(0.02), unit: "seconds" },
    ],
    outputs: &["out", "left", "right"],
    description: "One-pole smoothing per channel, time 0..60 seconds; zero is instantaneous. State resets to zero.",
    configuration: "none",
    example: "{\"kind\":\"smooth\"}",
    rust_variant: "Smooth",
};

pub const ENVELOPE: NodeInfo = NodeInfo {
    kind: "envelope",
    name: "Envelope",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "attack", default: Some(0.01), unit: "seconds" },
        PortInfo { name: "release", default: Some(0.1), unit: "seconds" },
    ],
    outputs: &["out", "left", "right"],
    description: "Per-channel peak follower, or exponential mean-square follower followed by sqrt when rms=true. Attack/release 0..60 seconds. State resets to zero.",
    configuration: " rms: bool",
    example: "{\"kind\":\"envelope\",\"rms\":false}",
    rust_variant: "Envelope",
};

pub const SAMPLE_HOLD: NodeInfo = NodeInfo {
    kind: "sample_hold",
    name: "Sample Hold",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "trigger", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Captures input on each nonpositive-to-positive trigger transition per channel. Held value and previous trigger reset to zero.",
    configuration: "none",
    example: "{\"kind\":\"sample_hold\"}",
    rust_variant: "SampleHold",
};

pub const FILTER: NodeInfo = NodeInfo {
    kind: "filter",
    name: "Filter",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "frequency", default: Some(1000.0), unit: "Hz" },
        PortInfo { name: "q", default: Some(std::f64::consts::FRAC_1_SQRT_2), unit: "Q" },
        PortInfo { name: "gain", default: Some(0.0), unit: "dB" },
    ],
    outputs: &["out", "left", "right"],
    description: "Stereo biquad using suhina-dsp; frequency 1..0.49*sample rate, Q 0.025..100, gain +/-48 dB. Independent channel state resets to zero. Modulation updates coefficients only when controls change.",
    configuration: " mode: FilterMode",
    example: "{\"kind\":\"filter\",\"mode\":\"lowpass\"}",
    rust_variant: "Filter",
};

pub const SAMPLE_DELAY: NodeInfo = NodeInfo {
    kind: "sample_delay",
    name: "Sample Delay",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "One-frame stereo delay; reads previous input before current input is committed. Breaks feedback cycles. Reset clears history.",
    configuration: "none",
    example: "{\"kind\":\"sample_delay\"}",
    rust_variant: "SampleDelay",
};

pub const DELAY: NodeInfo = NodeInfo {
    kind: "delay",
    name: "Delay",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "time", default: Some(0.0), unit: "seconds" },
        PortInfo { name: "mode", default: Some(2.0), unit: "0 repitch, 1 fade, 2 jump" },
    ],
    outputs: &["out", "left", "right"],
    description: "Fractional stereo delay with independent channel times and linear interpolation, time limited to one frame..max_seconds. Mode 0 slews time with a 20 ms one-pole (repitch), 1 crossfades read heads over 20 ms, 2 jumps to the new time (the default for older graphs). max_seconds must be >0 and <=60. Breaks feedback only through in port; time remains an ordinary dependency. Reset clears history.",
    configuration: " max_seconds: f64",
    example: "{\"kind\":\"delay\",\"max_seconds\":0.01}",
    rust_variant: "Delay",
};
