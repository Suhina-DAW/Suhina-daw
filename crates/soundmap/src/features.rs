//! The numbers that describe how a sample sounds (ADR 0013 › Features, version 1).
//!
//! Everything is computed from a mono sum of the first [`WINDOW_SECONDS`] of the file,
//! resampled to [`RATE`]. The layout of the vector is fixed by [`FEATURE_VERSION`]; the cache
//! drops entries of other versions.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};

/// Bumped whenever the list, the order or the computation of the features changes.
pub const FEATURE_VERSION: u32 = 1;
/// The number of features in version 1.
pub const FEATURE_COUNT: usize = 44;
/// How much of a file is analysed.
pub const WINDOW_SECONDS: f64 = 4.0;
/// The analysis sample rate.
pub const RATE: u32 = 22_050;

const FRAME: usize = 1024;
const HOP: usize = 256;
const MEL_BANDS: usize = 40;
const MFCCS: usize = 13;
/// Frames quieter than this below the loudest take no part in the spectral statistics.
const ACTIVE_RANGE_DB: f32 = 40.0;
const PITCH_HOP: usize = 1024;
const PITCH_MIN_HZ: f32 = 40.0;
const PITCH_MAX_HZ: f32 = 1000.0;
const YIN_THRESHOLD: f32 = 0.15;

/// Where each group of features sits in the vector, for weighting (ADR 0013 › Distances).
pub const GROUPS: [std::ops::Range<usize>; 4] = [0..26, 26..38, 38..42, 42..44];

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

    // MFCC mean and deviation.
    let filters = mel_filters();
    let mfccs: Vec<[f32; MFCCS]> =
        active.iter().filter_map(|&i| spectra.power.get(i)).map(|p| mfcc(p, &filters)).collect();
    for c in 0..MFCCS {
        let (mean, std) = mean_std(mfccs.iter().map(|m| m[c]));
        out[c] = mean;
        out[MFCCS + c] = std;
    }

    // Spectral shape, mean and deviation.
    let shapes: Vec<[f32; 5]> = active.iter().map(|&i| spectra.shape(i)).collect();
    for k in 0..5 {
        let (mean, std) = mean_std(shapes.iter().map(|s| s[k]));
        out[26 + k] = mean;
        out[31 + k] = std;
    }

    // Zero crossings, over the same frames.
    let zcr: Vec<f32> = active.iter().map(|&i| zero_crossing_rate(frame_of(&mono, i * HOP))).collect();
    let (mean, std) = mean_std(zcr.iter().copied());
    out[36] = mean;
    out[37] = std;

    // Envelope.
    let envelope = envelope(&mono);
    out[38..42].copy_from_slice(&envelope);

    // Pitch.
    let (pitch, voiced) = pitch(&mono);
    out[42] = pitch;
    out[43] = voiced;

    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// The first [`WINDOW_SECONDS`] as one channel (the mean of all channels).
fn mono_window(samples: &[f32], channels: u16, sample_rate: u32) -> Option<Vec<f32>> {
    let channels = usize::from(channels);
    if channels == 0 || sample_rate == 0 {
        return None;
    }
    let max_frames = (WINDOW_SECONDS * f64::from(sample_rate)) as usize;
    let scale = 1.0 / channels as f32;
    Some(samples.chunks_exact(channels).take(max_frames).map(|frame| frame.iter().sum::<f32>() * scale).collect())
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
        let flux = match i.checked_sub(1).and_then(|j| self.power.get(j)) {
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

/// Coefficients 1–13 of the DCT-II of the log mel energies.
fn mfcc(power: &[f32], filters: &[Vec<f32>]) -> [f32; MFCCS] {
    let log_mel: Vec<f32> = filters
        .iter()
        .map(|filter| {
            let energy: f32 = filter.iter().zip(power).map(|(w, p)| w * p).sum();
            10.0 * (energy + 1e-10).log10()
        })
        .collect();
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

/// Attack time, decay time and effective length (log10 seconds) and crest factor (dB).
fn envelope(signal: &[f32]) -> [f32; 4] {
    let block_seconds = HOP as f32 / RATE as f32;
    let levels: Vec<f32> =
        signal.chunks(HOP).map(|b| (b.iter().map(|s| s * s).sum::<f32>() / b.len().max(1) as f32).sqrt()).collect();
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
    let rms = (signal.iter().map(|s| s * s).sum::<f32>() / signal.len().max(1) as f32).sqrt().max(1e-9);
    let sample_peak = signal.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let crest = 20.0 * (sample_peak / rms).max(1.0).log10();
    let log = |seconds: f32| (seconds + 0.001).log10();
    [log(attack), log(decay), log(effective), crest]
}

/// The median fundamental of the voiced frames (as a MIDI note number, 0 when none is voiced)
/// and the share of loud frames that are voiced, by the YIN method.
fn pitch(signal: &[f32]) -> (f32, f32) {
    let tau_min = (RATE as f32 / PITCH_MAX_HZ) as usize;
    let tau_max = (RATE as f32 / PITCH_MIN_HZ) as usize;
    let width = FRAME - tau_max;
    let loudest = signal.chunks(PITCH_HOP).map(|b| b.iter().map(|s| s * s).sum::<f32>()).fold(0.0f32, f32::max);
    let floor = loudest * 10f32.powf(-ACTIVE_RANGE_DB / 10.0);
    let mut notes = Vec::new();
    let mut frames = 0usize;
    let mut start = 0;
    let mut difference = vec![0.0f32; tau_max + 1];
    while start + FRAME <= signal.len() {
        let Some(frame) = signal.get(start..start + FRAME) else { break };
        start += PITCH_HOP;
        let energy: f32 = frame.iter().take(PITCH_HOP).map(|s| s * s).sum();
        if energy < floor || energy <= 0.0 {
            continue;
        }
        frames += 1;
        for (tau, slot) in difference.iter_mut().enumerate() {
            *slot = frame.iter().take(width).zip(frame.iter().skip(tau)).map(|(a, b)| (a - b) * (a - b)).sum();
        }
        // Cumulative mean normalised difference.
        let mut running = 0.0;
        let mut normalised = vec![1.0f32; tau_max + 1];
        for tau in 1..=tau_max {
            let d = difference.get(tau).copied().unwrap_or(0.0);
            running += d;
            if let Some(slot) = normalised.get_mut(tau) {
                *slot = if running > 0.0 { d * tau as f32 / running } else { 1.0 };
            }
        }
        let mut found = None;
        let mut tau = tau_min.max(2);
        while tau < tau_max {
            let here = normalised.get(tau).copied().unwrap_or(1.0);
            if here < YIN_THRESHOLD {
                while tau + 1 < tau_max
                    && normalised.get(tau + 1).copied().unwrap_or(1.0) < normalised.get(tau).copied().unwrap_or(1.0)
                {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        if let Some(tau) = found {
            // Parabolic interpolation around the minimum.
            let (a, b, c) = (
                normalised.get(tau - 1).copied().unwrap_or(1.0),
                normalised.get(tau).copied().unwrap_or(1.0),
                normalised.get(tau + 1).copied().unwrap_or(1.0),
            );
            let denominator = a - 2.0 * b + c;
            let shift = if denominator.abs() > 1e-9 { 0.5 * (a - c) / denominator } else { 0.0 };
            let period = tau as f32 + shift.clamp(-1.0, 1.0);
            let hz = RATE as f32 / period;
            notes.push(69.0 + 12.0 * (hz / 440.0).log2());
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

fn mean_std(values: impl Iterator<Item = f32> + Clone) -> (f32, f32) {
    let (count, sum) = values.clone().fold((0usize, 0.0f32), |(n, s), v| (n + 1, s + v));
    if count == 0 {
        return (0.0, 0.0);
    }
    let mean = sum / count as f32;
    let variance = values.map(|v| (v - mean) * (v - mean)).sum::<f32>() / count as f32;
    (mean, variance.sqrt())
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
}
