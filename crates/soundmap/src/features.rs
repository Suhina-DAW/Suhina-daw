// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The numbers that describe how a sample sounds (ADR 0013 › Features, version 2).
//!
//! Everything is computed from a mono sum of the file, resampled to [`RATE`], from its onset
//! (leading silence is skipped) for [`WINDOW_SECONDS`]. The first [`ATTACK_SECONDS`] after the
//! onset (the attack) are also read on their own, apart from the rest (the body). The layout of
//! the vector is fixed by [`FEATURE_VERSION`]; the cache drops entries of other versions.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};

/// Bumped whenever the list, the order or the computation of the features changes.
pub const FEATURE_VERSION: u32 = 2;
/// The number of features in version 2.
pub const FEATURE_COUNT: usize = 57;
/// How much of a file is analysed, from its onset.
pub const WINDOW_SECONDS: f64 = 4.0;
/// How far into a file its onset is looked for; a file silent for longer is not analysed.
pub const ONSET_SEARCH_SECONDS: f64 = 4.0;
/// The analysis sample rate.
pub const RATE: u32 = 22_050;
/// The attack: this long from the onset.
pub const ATTACK_SECONDS: f32 = 0.030;
/// The upper edges of the first four of the [`BANDS`] energy bands, in Hz; the last band is
/// everything above 6 kHz.
pub const BAND_EDGES_HZ: [f32; BANDS - 1] = [120.0, 500.0, 2_000.0, 6_000.0];
pub const BANDS: usize = 5;

/// Where single features sit in the vector.
pub const PITCH: usize = 42;
pub const VOICED: usize = 43;
/// The low pitch's fall over the first [`GLIDE_SECONDS`], in semitones (positive when it falls).
pub const GLIDE: usize = 44;
/// The attack's band shares (dB), then its centroid (log2 Hz) and flatness.
pub const ATTACK_BANDS: std::ops::Range<usize> = 45..50;
pub const ATTACK_CENTROID: usize = 50;
pub const ATTACK_FLATNESS: usize = 51;
/// The body's band shares (dB).
pub const BODY_BANDS: std::ops::Range<usize> = 52..57;

const FRAME: usize = 1024;
const HOP: usize = 256;
const MEL_BANDS: usize = 40;
const MFCCS: usize = 13;
/// Frames quieter than this below the loudest take no part in the spectral statistics.
const ACTIVE_RANGE_DB: f32 = 40.0;
/// The range of the log mel energies under each frame's loudest band.
const MEL_RANGE_DB: f32 = 80.0;
/// The onset is the first moment within this of the loudest moment.
const ONSET_RANGE_DB: f32 = 40.0;
/// Blocks in which the onset is looked for, in seconds.
const ONSET_BLOCK_SECONDS: f64 = 0.001;
/// A band share is given in dB, never lower than this.
const SHARE_FLOOR_DB: f32 = -60.0;
/// The envelope's level is measured in blocks this long (about 3 ms).
const ENVELOPE_BLOCK: usize = 64;
const PITCH_HOP: usize = 1024;
const PITCH_MIN_HZ: f32 = 40.0;
const PITCH_MAX_HZ: f32 = 1000.0;
const YIN_THRESHOLD: f32 = 0.15;
/// How long from the onset the pitch glide is followed, the range its peak is looked for in, and
/// the share of a frame's energy that must lie under the top of that range.
pub const GLIDE_SECONDS: f32 = 0.25;
const GLIDE_MIN_HZ: f32 = 30.0;
const GLIDE_MAX_HZ: f32 = 500.0;
const GLIDE_LOW_SHARE: f32 = 0.5;
const MAX_GLIDE: f32 = 48.0;

/// Where each group of features sits in the vector, for weighting (ADR 0013 › Distances): MFCC,
/// spectral shape, envelope, pitch, attack, body bands.
pub const GROUPS: [std::ops::Range<usize>; 6] = [0..26, 26..38, 38..42, 42..45, 45..52, 52..57];
/// How much each group counts in distances, after it is scaled so that its size does not
/// matter: the spectrum (MFCC, shape, the bands of the attack and the body) counts most, the
/// envelope and the pitch less, so two short sharp hits are not near whatever their spectrum.
pub const GROUP_WEIGHTS: [f32; 6] = [1.0, 1.0, 0.5, 0.5, 1.0, 1.0];

/// A sample's features.
pub type Features = [f32; FEATURE_COUNT];

/// Computes the features of interleaved audio. `None` when there is nothing to analyse
/// (no frames, or silence throughout).
pub fn analyse(samples: &[f32], channels: u16, sample_rate: u32) -> Option<Features> {
    let mono = mono_window(samples, channels, sample_rate)?;
    let mono = resample(&mono, sample_rate, RATE);
    if mono.is_empty() || mono.iter().all(|s| s.abs() < 1e-6) || mono.iter().any(|s| !s.is_finite()) {
        return None;
    }
    let mut out = [0.0f32; FEATURE_COUNT];
    let spectra = Spectra::new(&mono);
    let active = spectra.active_frames();
    if active.is_empty() {
        return None;
    }

    // Each frame counts by its energy, so a quiet tail (a room's, or a release) counts little
    // beside the sound itself: one 10 dB down counts a tenth.
    let weights: Vec<f32> = active.iter().map(|&i| spectra.energy.get(i).copied().unwrap_or(0.0)).collect();

    // MFCC mean and deviation.
    let filters = mel_filters();
    let mfccs: Vec<[f32; MFCCS]> =
        active.iter().filter_map(|&i| spectra.power.get(i)).map(|p| mfcc(p, &filters)).collect();
    for c in 0..MFCCS {
        let (mean, std) = mean_std(mfccs.iter().map(|m| m[c]).zip(weights.iter().copied()));
        out[c] = mean;
        out[MFCCS + c] = std;
    }

    // Spectral shape, mean and deviation.
    let shapes: Vec<[f32; 5]> = active.iter().map(|&i| spectra.shape(i)).collect();
    for k in 0..5 {
        let (mean, std) = mean_std(shapes.iter().map(|s| s[k]).zip(weights.iter().copied()));
        out[26 + k] = mean;
        out[31 + k] = std;
    }

    // Zero crossings, over the same frames.
    let zcr: Vec<f32> = active.iter().map(|&i| zero_crossing_rate(frame_of(&mono, i * HOP))).collect();
    let (mean, std) = mean_std(zcr.iter().copied().zip(weights.iter().copied()));
    out[36] = mean;
    out[37] = std;

    // Envelope.
    let envelope = envelope(&mono);
    out[38..42].copy_from_slice(&envelope);

    // Pitch, and how it glides at the start.
    let (pitch, voiced) = pitch(&mono);
    out[PITCH] = pitch;
    out[VOICED] = voiced;
    out[GLIDE] = glide(&spectra, &active);

    // The attack on its own: its bands, centroid and flatness.
    let attack = attack_power(&mono);
    out[ATTACK_BANDS].copy_from_slice(&band_shares(&attack));
    let [centroid, _, flatness, _, _] = shape_of(&attack, None);
    out[ATTACK_CENTROID] = centroid;
    out[ATTACK_FLATNESS] = flatness;

    // The body's bands: the energy of the loud frames after the attack, summed, so the loudest
    // part counts most and a quiet tail little. A sound shorter than its attack uses all of it.
    let attack_frames = (ATTACK_SECONDS * RATE as f32) as usize / HOP;
    let body: Vec<usize> = active.iter().copied().filter(|&i| i >= attack_frames).collect();
    let body = if body.is_empty() { &active } else { &body };
    let mut summed = vec![0.0f32; FRAME / 2 + 1];
    for p in body.iter().filter_map(|&i| spectra.power.get(i)) {
        for (s, v) in summed.iter_mut().zip(p) {
            *s += v;
        }
    }
    out[BODY_BANDS].copy_from_slice(&band_shares(&summed));

    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// One channel (the mean of all channels) from the onset, for [`WINDOW_SECONDS`]. The onset is
/// the first millisecond within [`ONSET_RANGE_DB`] of the loudest one in the first
/// [`ONSET_SEARCH_SECONDS`] and the window after them; `None` when that is all silent.
fn mono_window(samples: &[f32], channels: u16, sample_rate: u32) -> Option<Vec<f32>> {
    let channels = usize::from(channels);
    if channels == 0 || sample_rate == 0 {
        return None;
    }
    let rate = f64::from(sample_rate);
    let max_frames = ((ONSET_SEARCH_SECONDS + WINDOW_SECONDS) * rate) as usize;
    let scale = 1.0 / channels as f32;
    let mono: Vec<f32> =
        samples.chunks_exact(channels).take(max_frames).map(|frame| frame.iter().sum::<f32>() * scale).collect();
    let block = ((ONSET_BLOCK_SECONDS * rate) as usize).max(1);
    let levels: Vec<f32> = mono.chunks(block).map(|b| b.iter().fold(0.0f32, |m, s| m.max(s.abs()))).collect();
    let loudest = levels.iter().copied().fold(0.0f32, f32::max);
    if loudest.is_nan() || loudest <= 0.0 {
        // Silent, or not a number: the caller refuses it.
        return Some(mono.into_iter().take((WINDOW_SECONDS * rate) as usize).collect());
    }
    let floor = loudest * 10f32.powf(-ONSET_RANGE_DB / 20.0);
    let search = ((ONSET_SEARCH_SECONDS * rate) as usize).div_ceil(block);
    let onset_block = levels.iter().take(search).position(|l| *l >= floor)?;
    let start = onset_block.saturating_sub(1) * block;
    Some(mono.into_iter().skip(start).take((WINDOW_SECONDS * rate) as usize).collect())
}

/// Resampling by linear interpolation, after a moving-average low-pass when the rate falls by
/// more than half. Coarse, but the features only look at broad spectral shape.
fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let smoothed;
    let source = if ratio >= 2.0 {
        let width = ratio.floor() as usize;
        let mut sum = 0.0f32;
        let mut out = Vec::with_capacity(input.len());
        for (i, s) in input.iter().enumerate() {
            sum += s;
            if i >= width {
                sum -= input.get(i - width).copied().unwrap_or(0.0);
            }
            out.push(sum / width.min(i + 1) as f32);
        }
        smoothed = out;
        &smoothed
    } else {
        input
    };
    let len = ((source.len() as f64) / ratio).floor() as usize;
    (0..len)
        .map(|i| {
            let position = i as f64 * ratio;
            let index = position.floor() as usize;
            let fraction = (position - index as f64) as f32;
            let a = source.get(index).copied().unwrap_or(0.0);
            let b = source.get(index + 1).copied().unwrap_or(a);
            a + (b - a) * fraction
        })
        .collect()
}

/// `FRAME` samples from `start`, zero-padded past the end.
fn frame_of(signal: &[f32], start: usize) -> &[f32] {
    let end = (start + FRAME).min(signal.len());
    signal.get(start..end).unwrap_or(&[])
}

struct Spectra {
    /// Power spectrum per frame, `FRAME / 2 + 1` bins.
    power: Vec<Vec<f32>>,
    /// Energy per frame (the sum of its power spectrum).
    energy: Vec<f32>,
}

impl Spectra {
    fn new(signal: &[f32]) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft: Arc<dyn RealToComplex<f32>> = planner.plan_fft_forward(FRAME);
        let window: Vec<f32> =
            (0..FRAME).map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / FRAME as f32).cos()).collect();
        let frames = signal.len().div_ceil(HOP).max(1);
        let mut input = fft.make_input_vec();
        let mut output: Vec<Complex32> = fft.make_output_vec();
        let mut scratch = fft.make_scratch_vec();
        let mut power = Vec::with_capacity(frames);
        let mut energy = Vec::with_capacity(frames);
        for f in 0..frames {
            let frame = frame_of(signal, f * HOP);
            for (n, slot) in input.iter_mut().enumerate() {
                *slot = frame.get(n).copied().unwrap_or(0.0) * window.get(n).copied().unwrap_or(0.0);
            }
            if fft.process_with_scratch(&mut input, &mut output, &mut scratch).is_err() {
                break;
            }
            let p: Vec<f32> = output.iter().map(|c| c.norm_sqr()).collect();
            energy.push(p.iter().sum());
            power.push(p);
        }
        Spectra { power, energy }
    }

    fn active_frames(&self) -> Vec<usize> {
        let loudest = self.energy.iter().copied().fold(0.0f32, f32::max);
        if loudest <= 0.0 {
            return Vec::new();
        }
        let floor = loudest * 10f32.powf(-ACTIVE_RANGE_DB / 10.0);
        self.energy.iter().enumerate().filter(|(_, e)| **e >= floor).map(|(i, _)| i).collect()
    }

    /// Centroid and spread (log2 Hz), flatness, roll-off (log2 Hz) and flux of frame `i`.
    fn shape(&self, i: usize) -> [f32; 5] {
        let Some(p) = self.power.get(i) else { return [0.0; 5] };
        shape_of(p, i.checked_sub(1).and_then(|j| self.power.get(j)).map(Vec::as_slice))
    }
}

/// Centroid and spread (log2 Hz), flatness, roll-off (log2 Hz) and, after `previous`, flux of a
/// power spectrum.
fn shape_of(p: &[f32], previous: Option<&[f32]>) -> [f32; 5] {
    let bin_hz = RATE as f32 / FRAME as f32;
    let total: f32 = p.iter().sum::<f32>().max(1e-12);
    let centroid = p.iter().enumerate().map(|(k, v)| k as f32 * bin_hz * v).sum::<f32>() / total;
    let spread =
        (p.iter().enumerate().map(|(k, v)| (k as f32 * bin_hz - centroid).powi(2) * v).sum::<f32>() / total).sqrt();
    let log_mean = p.iter().map(|v| (v + 1e-12).ln()).sum::<f32>() / p.len().max(1) as f32;
    let flatness = log_mean.exp() / (total / p.len().max(1) as f32);
    let mut running = 0.0;
    let mut rolloff = 0.0;
    for (k, v) in p.iter().enumerate() {
        running += v;
        if running >= 0.85 * total {
            rolloff = k as f32 * bin_hz;
            break;
        }
    }
    let flux = match previous {
        Some(previous) => {
            let previous_total: f32 = previous.iter().sum::<f32>().max(1e-12);
            p.iter()
                .zip(previous)
                .map(|(a, b)| ((a / total).sqrt() - (b / previous_total).sqrt()).max(0.0))
                .sum::<f32>()
        }
        None => 0.0,
    };
    [log2_hz(centroid), log2_hz(spread), flatness, log2_hz(rolloff), flux]
}

/// The power spectrum of the attack: its first [`ATTACK_SECONDS`] under a Hann window of that
/// length, zero-padded to a frame.
fn attack_power(signal: &[f32]) -> Vec<f32> {
    let length = ((ATTACK_SECONDS * RATE as f32) as usize).clamp(2, FRAME);
    let mut planner = RealFftPlanner::<f32>::new();
    let fft: Arc<dyn RealToComplex<f32>> = planner.plan_fft_forward(FRAME);
    let mut input = fft.make_input_vec();
    let mut output: Vec<Complex32> = fft.make_output_vec();
    for (n, slot) in input.iter_mut().enumerate().take(length) {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / (length - 1) as f32).cos();
        *slot = signal.get(n).copied().unwrap_or(0.0) * w;
    }
    if fft.process(&mut input, &mut output).is_err() {
        return vec![0.0; FRAME / 2 + 1];
    }
    output.iter().map(|c| c.norm_sqr()).collect()
}

/// The share of a power spectrum's energy in each of the [`BANDS`], in dB (0 dB: all of it).
fn band_shares(power: &[f32]) -> [f32; BANDS] {
    let bin_hz = RATE as f32 / FRAME as f32;
    let mut energy = [0.0f32; BANDS];
    for (k, v) in power.iter().enumerate() {
        let hz = k as f32 * bin_hz;
        let band = BAND_EDGES_HZ.iter().position(|edge| hz < *edge).unwrap_or(BANDS - 1);
        if let Some(slot) = energy.get_mut(band) {
            *slot += v;
        }
    }
    let total: f32 = energy.iter().sum();
    let mut out = [SHARE_FLOOR_DB; BANDS];
    if total > 0.0 {
        for (slot, e) in out.iter_mut().zip(energy) {
            *slot = (10.0 * (e / total).log10()).max(SHARE_FLOOR_DB);
        }
    }
    out
}

fn log2_hz(hz: f32) -> f32 {
    (hz.max(0.0) + 20.0).log2()
}

fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10f32.powf(mel / 2595.0) - 1.0)
}

/// Triangular mel filters over the power spectrum's bins.
fn mel_filters() -> Vec<Vec<f32>> {
    let bins = FRAME / 2 + 1;
    let bin_hz = RATE as f32 / FRAME as f32;
    let top = hz_to_mel(RATE as f32 / 2.0);
    let edges: Vec<f32> = (0..MEL_BANDS + 2).map(|i| mel_to_hz(top * i as f32 / (MEL_BANDS + 1) as f32)).collect();
    edges
        .windows(3)
        .map(|w| {
            let (low, centre, high) = (w[0], w[1], w[2]);
            (0..bins)
                .map(|k| {
                    let hz = k as f32 * bin_hz;
                    if hz <= low || hz >= high {
                        0.0
                    } else if hz <= centre {
                        (hz - low) / (centre - low).max(1e-6)
                    } else {
                        (high - hz) / (high - centre).max(1e-6)
                    }
                })
                .collect()
        })
        .collect()
}

/// Coefficients 1–13 of the DCT-II of the log mel energies. A band is never counted as more
/// than [`MEL_RANGE_DB`] below the frame's loudest band, so the coefficients do not change with
/// the level (only the zeroth would, and it is left out).
fn mfcc(power: &[f32], filters: &[Vec<f32>]) -> [f32; MFCCS] {
    let energies: Vec<f32> = filters.iter().map(|filter| filter.iter().zip(power).map(|(w, p)| w * p).sum()).collect();
    let floor = energies.iter().copied().fold(0.0f32, f32::max).max(1e-30) * 10f32.powf(-MEL_RANGE_DB / 10.0);
    let log_mel: Vec<f32> = energies.iter().map(|e| 10.0 * e.max(floor).log10()).collect();
    let n = log_mel.len().max(1) as f32;
    let mut out = [0.0f32; MFCCS];
    for (c, slot) in out.iter_mut().enumerate() {
        let k = (c + 1) as f32;
        let sum: f32 =
            log_mel.iter().enumerate().map(|(i, v)| v * (std::f32::consts::PI * k * (i as f32 + 0.5) / n).cos()).sum();
        *slot = sum * (2.0 / n).sqrt();
    }
    out
}

fn zero_crossing_rate(frame: &[f32]) -> f32 {
    if frame.len() < 2 {
        return 0.0;
    }
    let crossings = frame.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
    crossings as f32 / (frame.len() - 1) as f32
}

/// Attack time, decay time and effective length (log10 seconds) and crest factor (dB). The crest
/// factor is that of the sound until it has decayed by 20 dB, so a tail does not change it.
fn envelope(signal: &[f32]) -> [f32; 4] {
    let block_seconds = ENVELOPE_BLOCK as f32 / RATE as f32;
    let levels: Vec<f32> = signal
        .chunks(ENVELOPE_BLOCK)
        .map(|b| (b.iter().map(|s| s * s).sum::<f32>() / b.len().max(1) as f32).sqrt())
        .collect();
    let (peak_index, peak) =
        levels.iter().copied().enumerate().fold((0, 0.0f32), |best, (i, l)| if l > best.1 { (i, l) } else { best });
    if peak <= 0.0 {
        return [0.0; 4];
    }
    let floor_40 = peak * 0.01;
    let floor_20 = peak * 0.1;
    let start = levels.iter().position(|l| *l >= floor_40).unwrap_or(0);
    let attack = (peak_index.saturating_sub(start) + 1) as f32 * block_seconds;
    let decay_blocks =
        levels.iter().skip(peak_index).position(|l| *l < floor_20).unwrap_or(levels.len().saturating_sub(peak_index));
    let decay = decay_blocks.max(1) as f32 * block_seconds;
    let effective = levels.iter().filter(|l| **l >= floor_40).count() as f32 * block_seconds;
    let body = signal.get(..((peak_index + decay_blocks.max(1)) * ENVELOPE_BLOCK).min(signal.len())).unwrap_or(signal);
    let rms = (body.iter().map(|s| s * s).sum::<f32>() / body.len().max(1) as f32).sqrt().max(1e-9);
    let sample_peak = body.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let crest = 20.0 * (sample_peak / rms).max(1.0).log10();
    let log = |seconds: f32| (seconds + 0.001).log10();
    [log(attack), log(decay), log(effective), crest]
}

/// The median fundamental of the voiced frames (as a MIDI note number, 0 when none is voiced)
/// and the share of loud frames that are voiced, by the YIN method.
fn pitch(signal: &[f32]) -> (f32, f32) {
    let mut yin = Yin::new();
    let loudest = signal.chunks(PITCH_HOP).map(|b| b.iter().map(|s| s * s).sum::<f32>()).fold(0.0f32, f32::max);
    let floor = loudest * 10f32.powf(-ACTIVE_RANGE_DB / 10.0);
    let mut notes = Vec::new();
    let mut frames = 0usize;
    let mut start = 0;
    while start + FRAME <= signal.len() {
        let Some(frame) = signal.get(start..start + FRAME) else { break };
        start += PITCH_HOP;
        let energy: f32 = frame.iter().take(PITCH_HOP).map(|s| s * s).sum();
        if energy < floor || energy <= 0.0 {
            continue;
        }
        frames += 1;
        if let Some(note) = yin.note(frame) {
            notes.push(note);
        }
    }
    if notes.is_empty() || frames == 0 {
        return (0.0, 0.0);
    }
    let voiced = notes.len() as f32 / frames as f32;
    notes.sort_by(f32::total_cmp);
    let median = notes.get(notes.len() / 2).copied().unwrap_or(0.0);
    (median, voiced)
}

/// How far the low pitch falls over the first [`GLIDE_SECONDS`] (the sweep of a kick or a
/// tom), in semitones: the strongest peak under [`GLIDE_MAX_HZ`] in the first loud frame less
/// the one in the last, over the frames whose energy lies mostly below it. 0 when fewer than two
/// frames do.
fn glide(spectra: &Spectra, active: &[usize]) -> f32 {
    let bin_hz = RATE as f32 / FRAME as f32;
    let last_frame = (GLIDE_SECONDS * RATE as f32) as usize / HOP;
    let top = (GLIDE_MAX_HZ / bin_hz) as usize;
    let low = (GLIDE_MIN_HZ / bin_hz).ceil() as usize;
    let notes: Vec<f32> = active
        .iter()
        .filter(|&&i| i <= last_frame)
        .filter_map(|&i| spectra.power.get(i))
        .filter_map(|p| {
            let below: f32 = p.iter().take(top + 1).sum();
            if below < GLIDE_LOW_SHARE * p.iter().sum::<f32>() || below <= 0.0 {
                return None;
            }
            let (k, _) = p
                .iter()
                .enumerate()
                .take(top + 1)
                .skip(low)
                .fold((0, 0.0f32), |best, (k, v)| if *v > best.1 { (k, *v) } else { best });
            // Parabolic interpolation on the log power around the peak.
            let at = |j: usize| (p.get(j).copied().unwrap_or(0.0) + 1e-12).ln();
            let (a, b, c) = (at(k.saturating_sub(1)), at(k), at(k + 1));
            let denominator = a - 2.0 * b + c;
            let shift = if denominator.abs() > 1e-9 { 0.5 * (a - c) / denominator } else { 0.0 };
            let hz = (k as f32 + shift.clamp(-0.5, 0.5)) * bin_hz;
            (hz > 0.0).then(|| 12.0 * (hz / 440.0).log2())
        })
        .collect();
    match (notes.first(), notes.last()) {
        (Some(first), Some(last)) if notes.len() >= 2 => (first - last).clamp(-MAX_GLIDE, MAX_GLIDE),
        _ => 0.0,
    }
}

/// A YIN pitch estimate of one frame, with its working buffers.
struct Yin {
    tau_min: usize,
    tau_max: usize,
    difference: Vec<f32>,
    normalised: Vec<f32>,
}

impl Yin {
    fn new() -> Self {
        let tau_max = (RATE as f32 / PITCH_MIN_HZ) as usize;
        Yin {
            tau_min: (RATE as f32 / PITCH_MAX_HZ) as usize,
            tau_max,
            difference: vec![0.0; tau_max + 1],
            normalised: vec![1.0; tau_max + 1],
        }
    }

    /// The frame's fundamental as a MIDI note number, when it is voiced. `frame` is [`FRAME`]
    /// samples long.
    fn note(&mut self, frame: &[f32]) -> Option<f32> {
        let (tau_min, tau_max) = (self.tau_min, self.tau_max);
        let width = FRAME.saturating_sub(tau_max);
        for (tau, slot) in self.difference.iter_mut().enumerate() {
            *slot = frame.iter().take(width).zip(frame.iter().skip(tau)).map(|(a, b)| (a - b) * (a - b)).sum();
        }
        // Cumulative mean normalised difference.
        let normalised = &mut self.normalised;
        let mut running = 0.0;
        for tau in 1..=tau_max {
            let d = self.difference.get(tau).copied().unwrap_or(0.0);
            running += d;
            if let Some(slot) = normalised.get_mut(tau) {
                *slot = if running > 0.0 { d * tau as f32 / running } else { 1.0 };
            }
        }
        let value = |tau: usize| normalised.get(tau).copied().unwrap_or(1.0);
        let mut tau = tau_min.max(2);
        let mut found = None;
        while tau < tau_max {
            if value(tau) < YIN_THRESHOLD {
                while tau + 1 < tau_max && value(tau + 1) < value(tau) {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = found?;
        // Parabolic interpolation around the minimum.
        let (a, b, c) = (value(tau - 1), value(tau), value(tau + 1));
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1e-9 { 0.5 * (a - c) / denominator } else { 0.0 };
        let period = tau as f32 + shift.clamp(-1.0, 1.0);
        let hz = RATE as f32 / period;
        Some(69.0 + 12.0 * (hz / 440.0).log2())
    }
}

/// The weighted mean and standard deviation of `(value, weight)` pairs.
fn mean_std(values: impl Iterator<Item = (f32, f32)> + Clone) -> (f32, f32) {
    let (total, sum) = values.clone().fold((0.0f32, 0.0f32), |(w, s), (v, weight)| (w + weight, s + v * weight));
    if total.is_nan() || total <= 0.0 {
        return (0.0, 0.0);
    }
    let mean = sum / total;
    let variance = values.map(|(v, w)| (v - mean) * (v - mean) * w).sum::<f32>() / total;
    (mean, variance.max(0.0).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, seconds: f32, rate: u32) -> Vec<f32> {
        let n = (seconds * rate as f32) as usize;
        (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * 0.5).collect()
    }

    /// A deterministic noise source, so tests do not depend on a random crate.
    fn noise(seconds: f32, rate: u32) -> Vec<f32> {
        let mut state = 0x1234_5678u32;
        (0..(seconds * rate as f32) as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32) - 0.5
            })
            .collect()
    }

    #[test]
    fn silence_and_empty_input_have_no_features() {
        assert!(analyse(&[], 1, 44_100).is_none());
        assert!(analyse(&[0.0; 44_100], 1, 44_100).is_none());
        assert!(analyse(&[0.5; 100], 0, 44_100).is_none());
        assert!(analyse(&[f32::NAN; 4096], 1, 44_100).is_none());
    }

    #[test]
    fn a_sine_has_its_pitch_and_a_low_flatness() {
        let features = analyse(&sine(220.0, 1.0, 44_100), 1, 44_100).unwrap();
        // A3 is MIDI note 57.
        assert!((features[42] - 57.0).abs() < 0.3, "pitch {}", features[42]);
        assert!(features[43] > 0.9, "voiced share {}", features[43]);
        assert!(features[28] < 0.05, "flatness {}", features[28]);
        // The centroid sits near 220 Hz (log2 of 240).
        assert!((features[26] - 240f32.log2()).abs() < 0.5, "centroid {}", features[26]);
    }

    #[test]
    fn noise_is_flat_bright_and_unvoiced() {
        let noisy = analyse(&noise(1.0, 44_100), 1, 44_100).unwrap();
        let tone = analyse(&sine(220.0, 1.0, 44_100), 1, 44_100).unwrap();
        assert!(noisy[28] > 0.3, "flatness {}", noisy[28]);
        assert!(noisy[26] > tone[26] + 2.0, "centroid {} against {}", noisy[26], tone[26]);
        assert!(noisy[43] < 0.2, "voiced share {}", noisy[43]);
        assert!(noisy[36] > tone[36] * 10.0, "zero crossings {} against {}", noisy[36], tone[36]);
    }

    #[test]
    fn a_short_hit_has_a_short_effective_length() {
        let rate = 44_100;
        let mut hit: Vec<f32> = noise(0.05, rate);
        hit.resize(rate as usize, 0.0);
        let long = analyse(&noise(1.0, rate), 1, rate).unwrap();
        let short = analyse(&hit, 1, rate).unwrap();
        assert!(short[40] < long[40] - 0.5, "length {} against {}", short[40], long[40]);
    }

    #[test]
    fn stereo_and_rate_do_not_change_much() {
        let mono = sine(440.0, 1.0, 44_100);
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        let a = analyse(&mono, 1, 44_100).unwrap();
        let b = analyse(&stereo, 2, 44_100).unwrap();
        assert_eq!(a, b);
        let c = analyse(&sine(440.0, 1.0, 48_000), 1, 48_000).unwrap();
        assert!((a[42] - c[42]).abs() < 0.2);
        assert!((a[26] - c[26]).abs() < 0.2);
    }

    #[test]
    fn only_the_window_is_read() {
        let mut long = sine(220.0, WINDOW_SECONDS as f32, 22_050);
        let first = analyse(&long, 1, 22_050).unwrap();
        long.extend(noise(2.0, 22_050));
        assert_eq!(analyse(&long, 1, 22_050).unwrap(), first);
    }

    /// A sine whose pitch falls from `from` to `to` Hz over 0.1 s, then holds, fading out.
    fn sweep(from: f32, to: f32, seconds: f32, rate: u32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..(seconds * rate as f32) as usize)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let hz = to + (from - to) * (-t * 30.0).exp();
                phase += std::f32::consts::TAU * hz / rate as f32;
                phase.sin() * (-t * 4.0).exp() * 0.8
            })
            .collect()
    }

    #[test]
    fn leading_silence_is_skipped() {
        let hit = sweep(150.0, 50.0, 0.5, 44_100);
        let mut late = vec![0.0f32; 44_100];
        // A faint floor, as a recording has, 80 dB down.
        late.iter_mut().enumerate().for_each(|(i, s)| *s = if i % 2 == 0 { 1e-4 } else { -1e-4 });
        late.extend_from_slice(&hit);
        let a = analyse(&hit, 1, 44_100).unwrap();
        let b = analyse(&late, 1, 44_100).unwrap();
        // The frames fall a little differently over the sound, so the numbers move a little.
        for i in 0..FEATURE_COUNT {
            assert!((a[i] - b[i]).abs() <= 0.15 * a[i].abs().max(3.0), "feature {i}: {} against {}", a[i], b[i]);
        }
        // A file silent for longer than the onset is looked for has nothing to analyse.
        let mut silent = vec![0.0f32; (ONSET_SEARCH_SECONDS as usize + 1) * 44_100];
        silent.extend_from_slice(&hit);
        assert!(analyse(&silent, 1, 44_100).is_none());
    }

    #[test]
    fn the_level_does_not_matter() {
        let hit = sweep(150.0, 50.0, 0.5, 44_100);
        let quiet: Vec<f32> = hit.iter().map(|s| s * 0.01).collect();
        let a = analyse(&hit, 1, 44_100).unwrap();
        let b = analyse(&quiet, 1, 44_100).unwrap();
        for i in 0..FEATURE_COUNT {
            assert!((a[i] - b[i]).abs() <= 0.02 * a[i].abs().max(1.0), "feature {i}: {} against {}", a[i], b[i]);
        }
    }

    #[test]
    fn band_shares_say_where_the_energy_is() {
        let low = analyse(&sine(60.0, 1.0, 44_100), 1, 44_100).unwrap();
        // Nearly all of a 60 Hz sine is under 120 Hz.
        assert!(low[BODY_BANDS.start] > -0.5, "{:?}", &low[BODY_BANDS]);
        assert!(low[BODY_BANDS.end - 1] < -40.0, "{:?}", &low[BODY_BANDS]);
        let bright = analyse(&noise(1.0, 44_100), 1, 44_100).unwrap();
        // White noise: most of it above 6 kHz, little under 120 Hz.
        let top = bright[BODY_BANDS.end - 1];
        assert!(top > -5.0 && bright[BODY_BANDS.start] < -15.0, "{:?}", &bright[BODY_BANDS]);
        assert!(bright[ATTACK_BANDS.start] < -10.0, "{:?}", &bright[ATTACK_BANDS]);
        // Each band is a share: the shares add up to the whole.
        let total: f32 = bright[BODY_BANDS].iter().map(|db| 10f32.powf(db / 10.0)).sum();
        assert!((total - 1.0).abs() < 0.01, "{total}");
    }

    #[test]
    fn the_attack_is_read_apart_from_the_body() {
        // A bright click, then a low tone.
        let rate = 44_100;
        let mut hit: Vec<f32> = noise(0.02, rate).iter().map(|s| s * 2.0).collect();
        hit.extend(sine(60.0, 0.6, rate));
        let f = analyse(&hit, 1, rate).unwrap();
        assert!(f[ATTACK_CENTROID] > f[26] + 1.0, "attack centroid {} against {}", f[ATTACK_CENTROID], f[26]);
        assert!(f[ATTACK_BANDS.start] < f[BODY_BANDS.start] - 6.0, "{:?} {:?}", &f[ATTACK_BANDS], &f[BODY_BANDS]);
        assert!(f[ATTACK_FLATNESS] > 0.1, "attack flatness {}", f[ATTACK_FLATNESS]);
    }

    #[test]
    fn a_falling_pitch_has_a_glide_and_a_steady_one_none() {
        let kick = analyse(&sweep(160.0, 50.0, 0.5, 44_100), 1, 44_100).unwrap();
        // 160 Hz to 50 Hz is about 20 semitones; the first frame already averages some of the fall.
        assert!(kick[GLIDE] > 8.0, "glide {}", kick[GLIDE]);
        let steady = analyse(&sine(55.0, 0.5, 44_100), 1, 44_100).unwrap();
        assert!(steady[GLIDE].abs() < 1.0, "glide {}", steady[GLIDE]);
        let hiss = analyse(&noise(0.5, 44_100), 1, 44_100).unwrap();
        assert_eq!(hiss[GLIDE], 0.0);
    }

    #[test]
    fn a_tail_changes_the_crest_factor_little() {
        let rate = 44_100;
        let hit = sweep(150.0, 50.0, 0.3, rate);
        let mut tailed = hit.clone();
        // A long quiet tail, 30 dB down.
        tailed.extend(noise(1.5, rate).iter().map(|s| s * 0.03));
        let a = analyse(&hit, 1, rate).unwrap();
        let b = analyse(&tailed, 1, rate).unwrap();
        assert!((a[41] - b[41]).abs() < 1.0, "crest {} against {}", a[41], b[41]);
    }
}
