//! Audio clip playback. Each clip placement becomes an [`AudioEvent`] when the renderer is built;
//! `process` only reads decoded samples that are already in memory.
//!
//! Warped clips follow their warp markers: a beat in the clip maps to a time in the file, piece
//! by piece. There is no time-stretching yet, so a clip that plays faster or slower than its file
//! also sounds higher or lower ("re-pitch"); transpose applies to unwarped clips only.

use std::collections::HashMap;
use std::sync::Arc;

use loopwerk_model::{AudioRegion, Clip};

/// File sample rates the direct reader plays; a file outside them is not read (a rate of 0 would
/// freeze on one frame). Wider than [`STRETCH_SAMPLE_RATES`], since the direct reader is where a
/// clip the stretcher refuses goes.
pub(crate) const FILE_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 1_000..=384_000;
/// File sample rates the stretcher takes.
pub(crate) const STRETCH_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// Decoded audio, interleaved f32.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AudioData {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// The stereo sample at `seconds` into the file, linearly interpolated; silence outside it.
    fn stereo_at(&self, seconds: f64) -> (f32, f32) {
        self.frame_at(seconds * f64::from(self.sample_rate))
    }

    /// The stereo sample at frame `position` of the file (fractional frames interpolate
    /// linearly); silence outside it. Mono plays on both sides.
    pub(crate) fn frame_at(&self, position: f64) -> (f32, f32) {
        let channels = usize::from(self.channels.max(1));
        if position.is_nan() || position < 0.0 {
            return (0.0, 0.0);
        }
        let index = position.floor() as usize;
        let fraction = (position - position.floor()) as f32;
        let frame = |i: usize| -> (f32, f32) {
            let base = i.saturating_mul(channels);
            let left = self.samples.get(base).copied().unwrap_or(0.0);
            let right = if channels > 1 { self.samples.get(base + 1).copied().unwrap_or(0.0) } else { left };
            (left, right)
        };
        let (l0, r0) = frame(index);
        let (l1, r1) = frame(index.saturating_add(1));
        (l0 + (l1 - l0) * fraction, r0 + (r1 - r0) * fraction)
    }
}

/// The audio a project needs, prepared before playback.
#[derive(Debug, Clone, Default)]
pub struct AudioPool {
    /// Decoded files, keyed by the `file` string of the project's audio regions.
    pub files: HashMap<String, Arc<AudioData>>,
}

impl AudioPool {
    pub fn new() -> Self {
        Self::default()
    }
}

/// How a beat in the clip maps to seconds in the file.
#[derive(Debug, Clone, PartialEq)]
enum TimeMap {
    /// Unwarped, or warped without usable markers: a fixed rate.
    Linear { seconds_per_beat: f64 },
    /// Warp markers as (beat, seconds), sorted by beat, at least two.
    Warp(Vec<(f64, f64)>),
}

impl TimeMap {
    fn seconds(&self, beat: f64) -> f64 {
        match self {
            Self::Linear { seconds_per_beat } => beat * seconds_per_beat,
            Self::Warp(markers) => {
                // The segment that contains `beat`, or the first or last one to extend.
                let i =
                    markers.iter().position(|(b, _)| *b > beat).unwrap_or(markers.len()).clamp(1, markers.len() - 1);
                let (Some(&(b0, s0)), Some(&(b1, s1))) = (markers.get(i - 1), markers.get(i)) else { return 0.0 };
                if b1 <= b0 {
                    return s0;
                }
                s0 + (beat - b0) * (s1 - s0) / (b1 - b0)
            }
        }
    }
}

/// How loud a clip is near its edges: a short click guard at both ends, times the clip's own
/// fade-in and fade-out (`docs/spec/arrangement-editing.md` › Fades). All lengths are in frames;
/// the fades are linear in amplitude and multiply with the guard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fades {
    click: f64,
    fade_in: f64,
    fade_out: f64,
}

/// The click guard at both ends of every audio clip, in seconds.
pub(crate) const CLICK_GUARD_SECONDS: f64 = 0.002;

impl Fades {
    /// The fades of `clip` at `frames_per_beat`, fitted to the clip (a malformed project cannot
    /// make them negative or longer than the clip).
    pub(crate) fn new(clip: &Clip, frames_per_beat: f64, sample_rate: u32) -> Self {
        let (fade_in, fade_out) = clip.fitted_fades();
        let frames = |beats: f64| {
            let f = beats * frames_per_beat;
            if f.is_finite() { f.max(0.0) } else { 0.0 }
        };
        Self {
            click: (CLICK_GUARD_SECONDS * f64::from(sample_rate)).max(1.0),
            fade_in: frames(fade_in),
            fade_out: frames(fade_out),
        }
    }

    /// The gain `since_start` frames after the clip starts and `until_end` frames before it
    /// ends (the end is the frame after the last), 0 … 1. The ramps count the frames before and
    /// after this one, so the first and the last frame are both 0.
    pub(crate) fn gain(&self, since_start: f64, until_end: f64) -> f32 {
        let until_end = until_end - 1.0;
        let guard = (since_start / self.click).min(until_end / self.click);
        let fade_in = if self.fade_in > 0.0 { since_start / self.fade_in } else { 1.0 };
        let fade_out = if self.fade_out > 0.0 { until_end / self.fade_out } else { 1.0 };
        (guard.clamp(0.0, 1.0) * fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)) as f32
    }
}

/// The project timing an audio event is resolved against.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    pub frames_per_beat: f64,
    pub tempo_bpm: f64,
    pub sample_rate: u32,
}

/// One audio clip placement, resolved to frames.
#[derive(Debug, Clone)]
pub(crate) struct AudioEvent {
    pub start: u64,
    pub end: u64,
    frames_per_beat: f64,
    /// Where playback starts in the clip, in clip beats.
    offset: f64,
    loop_range: Option<(f64, f64)>,
    /// Clip beats advanced per project beat (transpose on unwarped clips).
    rate: f64,
    map: TimeMap,
    data: Arc<AudioData>,
    gain_left: f32,
    gain_right: f32,
    fades: Fades,
    /// Whether the clip follows the beat (warped); an unwarped clip plays at its own speed.
    warped: bool,
    /// Seconds added to the file position: an unwarped clip that was playing when the tempo
    /// changed carries on from where it was (ADR 0005).
    shift: f64,
}

impl AudioEvent {
    pub(crate) fn new(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        timing: Timing,
        gains: (f32, f32),
    ) -> Option<Self> {
        let Timing { frames_per_beat, tempo_bpm, sample_rate } = timing;
        let finite = [clip.start, clip.length, clip.offset, frames_per_beat, tempo_bpm].iter().all(|v| v.is_finite());
        if !finite
            || clip.length <= 0.0
            || frames_per_beat <= 0.0
            || tempo_bpm <= 0.0
            || data.frames() == 0
            || !FILE_SAMPLE_RATES.contains(&data.sample_rate)
        {
            return None;
        }
        let linear = TimeMap::Linear { seconds_per_beat: 60.0 / tempo_bpm };
        let mut markers: Vec<(f64, f64)> = region
            .warp_markers
            .iter()
            .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
            .map(|m| (m.time, m.audio_time))
            .collect();
        markers.sort_by(|a, b| a.0.total_cmp(&b.0));
        markers.dedup_by(|a, b| a.0 == b.0);
        let (map, rate) = if region.warped {
            (if markers.len() >= 2 { TimeMap::Warp(markers) } else { linear }, 1.0)
        } else {
            let rate = 2f64.powf(region.transpose / 12.0);
            (linear, if rate.is_finite() && rate > 0.0 { rate } else { 1.0 })
        };
        let gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
        let to_frame = |beats: f64| (beats.max(0.0) * frames_per_beat).round() as u64;
        Some(Self {
            start: to_frame(clip.start),
            end: to_frame(clip.start + clip.length),
            frames_per_beat,
            offset: clip.offset,
            loop_range: clip
                .loop_range
                .filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start)
                .map(|r| (r.start, r.end)),
            rate,
            map,
            data,
            gain_left: gains.0 * gain,
            gain_right: gains.1 * gain,
            fades: Fades::new(clip, frames_per_beat, sample_rate),
            warped: region.warped,
            shift: 0.0,
        })
    }

    /// The gain envelope at the clip's edges.
    pub(crate) fn fades(&self) -> Fades {
        self.fades
    }

    pub(crate) fn warped(&self) -> bool {
        self.warped
    }

    /// The file position, in seconds, played at frame `position` (inside `start..end`).
    pub(crate) fn file_seconds(&self, position: u64) -> f64 {
        let since_start = position.saturating_sub(self.start) as f64;
        self.map.seconds(self.clip_beat(since_start / self.frames_per_beat)) + self.shift
    }

    /// Makes the clip play file position `seconds` at frame `position`, and on from there.
    pub(crate) fn continue_from(&mut self, position: u64, seconds: f64) {
        self.shift = 0.0;
        let shift = seconds - self.file_seconds(position);
        self.shift = if shift.is_finite() { shift } else { 0.0 };
    }

    /// The clip beat played `elapsed` project beats after the clip starts: from the offset, then
    /// around the loop if there is one.
    fn clip_beat(&self, elapsed: f64) -> f64 {
        let beat = self.offset + elapsed * self.rate;
        match self.loop_range {
            Some((start, end)) if beat >= end => start + (beat - end).rem_euclid(end - start),
            _ => beat,
        }
    }

    /// The stereo sample at frame `position`, which must be inside `start..end`.
    pub(crate) fn sample(&self, position: u64) -> (f32, f32) {
        let since_start = position.saturating_sub(self.start) as f64;
        let until_end = self.end.saturating_sub(position) as f64;
        let fade = self.fades.gain(since_start, until_end);
        let seconds = self.map.seconds(self.clip_beat(since_start / self.frames_per_beat)) + self.shift;
        let (left, right) = self.data.stereo_at(seconds);
        (left * self.gain_left * fade, right * self.gain_right * fade)
    }

    /// Like [`AudioEvent::sample`] for an event placed at frame 0, at a fractional number of
    /// frames since it started; silence outside `0..end`. Launched clips use this, since their
    /// position follows the launcher clock rather than a fixed frame.
    pub(crate) fn sample_elapsed(&self, elapsed_frames: f64) -> (f32, f32) {
        let until_end = self.end as f64 - elapsed_frames;
        if !(elapsed_frames >= 0.0 && until_end > 0.0) {
            return (0.0, 0.0);
        }
        let fade = self.fades.gain(elapsed_frames, until_end);
        let seconds = self.map.seconds(self.clip_beat(elapsed_frames / self.frames_per_beat));
        let (left, right) = self.data.stereo_at(seconds);
        (left * self.gain_left * fade, right * self.gain_right * fade)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loopwerk_model::*;

    const SR: u32 = 48_000;

    /// One second of a ramp, so the sample value tells the file position: value = seconds.
    fn ramp() -> Arc<AudioData> {
        Arc::new(AudioData { sample_rate: 8000, channels: 1, samples: (0..8000).map(|i| i as f32 / 8000.0).collect() })
    }

    fn region(warped: bool, markers: &[(f64, f64)], transpose: f64) -> AudioRegion {
        AudioRegion {
            file: "ramp.wav".into(),
            absolute_file: None,
            sample_rate: Some(8000),
            channels: Some(1),
            duration_seconds: Some(1.0),
            warped,
            stretch: StretchMode::Smooth,
            warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
            gain: 1.0,
            transpose,
            stream: None,
        }
    }

    fn clip(length: f64, offset: f64, loop_range: Option<(f64, f64)>) -> Clip {
        Clip {
            content: ContentId(1),
            start: 0.0,
            length,
            offset,
            loop_range: loop_range.map(|(start, end)| LoopRange { start, end }),
            enabled: true,
            fade_in: 0.0,
            fade_out: 0.0,
            extensions: Extensions::default(),
        }
    }

    /// File position (seconds) played at `beat` in the clip, at 120 bpm (0.5 s per beat).
    fn file_seconds_at(event: &AudioEvent, beat: f64) -> f32 {
        event.sample((beat * event.frames_per_beat) as u64).0
    }

    fn event(clip: &Clip, region: &AudioRegion) -> AudioEvent {
        AudioEvent::new(
            clip,
            region,
            ramp(),
            Timing { frames_per_beat: 60.0 / 120.0 * f64::from(SR), tempo_bpm: 120.0, sample_rate: SR },
            (1.0, 1.0),
        )
        .unwrap()
    }

    #[test]
    fn fades_are_fitted_to_the_clip_and_meet_the_click_guard() {
        // 4 beats of fades on a 2-beat clip are fitted to 1.5 + 0.5 beats, at 100 frames a beat.
        let c = Clip { fade_in: 3.0, fade_out: 1.0, ..clip(2.0, 0.0, None) };
        let fades = Fades::new(&c, 100.0, 1000);
        assert_eq!((fades.fade_in, fades.fade_out), (150.0, 50.0));
        assert_eq!(fades.click, 2.0);
        // Half-way into the fade-in; the guard is over after two frames.
        assert!((fades.gain(75.0, 125.0) - 0.5).abs() < 1e-6);
        assert!((fades.gain(1.0, 199.0) - 0.5 / 150.0).abs() < 1e-6, "guard and fade multiply");
        assert!((fades.gain(160.0, 40.0) - 0.78).abs() < 1e-6, "39 frames after this one, of 50");
        assert_eq!(fades.gain(100.0, 1.0), 0.0, "the last frame");
        assert_eq!(fades.gain(0.0, 200.0), 0.0);
        let c = Clip { fade_in: f64::NAN, fade_out: -1.0, ..clip(2.0, 0.0, None) };
        assert_eq!(Fades::new(&c, 100.0, 1000).gain(100.0, 100.0), 1.0, "bad fades are no fades");
    }

    /// The direct reader refuses a tempo of 0 and a file sample rate outside 1 to 384 kHz, as the
    /// stretcher refuses them (#164).
    #[test]
    fn a_zero_tempo_or_an_unplayable_sample_rate_is_no_event() {
        let timing = |tempo_bpm: f64| Timing { frames_per_beat: 24_000.0, tempo_bpm, sample_rate: SR };
        let c = clip(2.0, 0.0, None);
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(0.0), (1.0, 1.0)).is_none());
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(-120.0), (1.0, 1.0)).is_none());
        for rate in [0, 999, 384_001, 1_000_000] {
            let data = Arc::new(AudioData { sample_rate: rate, ..(*ramp()).clone() });
            assert!(AudioEvent::new(&c, &region(false, &[], 0.0), data, timing(120.0), (1.0, 1.0)).is_none(), "{rate}");
        }
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(120.0), (1.0, 1.0)).is_some());
    }

    #[test]
    fn unwarped_plays_at_file_speed() {
        let e = event(&clip(2.0, 0.0, None), &region(false, &[], 0.0));
        assert!((file_seconds_at(&e, 1.0) - 0.5).abs() < 1e-3);
        assert!((file_seconds_at(&e, 1.5) - 0.75).abs() < 1e-3);
    }

    #[test]
    fn transpose_speeds_up_unwarped_clips() {
        let e = event(&clip(2.0, 0.0, None), &region(false, &[], 12.0));
        assert!((file_seconds_at(&e, 0.5) - 0.5).abs() < 1e-3, "an octave up plays twice as fast");
    }

    #[test]
    fn warp_markers_map_beats_to_file_time() {
        // Beat 0 at 0 s, beat 2 at 0.5 s: the file plays at half speed against 120 bpm.
        let e = event(&clip(2.0, 0.0, None), &region(true, &[(0.0, 0.0), (2.0, 0.5)], 12.0));
        assert!((file_seconds_at(&e, 1.0) - 0.25).abs() < 1e-3);
        // Past the last marker the last segment continues.
        let e = event(&clip(4.0, 0.0, None), &region(true, &[(0.0, 0.0), (2.0, 0.5)], 0.0));
        assert!((file_seconds_at(&e, 3.0) - 0.75).abs() < 1e-3);
    }

    #[test]
    fn offset_and_loop() {
        // Start at clip beat 1 (0.5 s), loop beats 0..1.5 (0..0.75 s).
        let e = event(&clip(4.0, 1.0, Some((0.0, 1.5))), &region(false, &[], 0.0));
        assert!((file_seconds_at(&e, 0.25) - 0.625).abs() < 1e-3);
        // After 0.5 beats the loop end is reached and playback wraps to its start.
        assert!((file_seconds_at(&e, 0.75) - 0.125).abs() < 1e-3);
    }

    #[test]
    fn outside_the_file_is_silent_and_bad_clips_are_rejected() {
        let e = event(&clip(8.0, 0.0, None), &region(false, &[], 0.0));
        assert_eq!(e.sample(e.start + (3.0 * e.frames_per_beat) as u64), (0.0, 0.0));
        let fpb = 24_000.0;
        assert!(
            AudioEvent::new(
                &clip(0.0, 0.0, None),
                &region(false, &[], 0.0),
                ramp(),
                Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR },
                (1.0, 1.0)
            )
            .is_none()
        );
        assert!(
            AudioEvent::new(
                &clip(f64::NAN, 0.0, None),
                &region(false, &[], 0.0),
                ramp(),
                Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR },
                (1.0, 1.0)
            )
            .is_none()
        );
        let empty = Arc::new(AudioData { sample_rate: 1000, channels: 2, samples: Vec::new() });
        assert!(
            AudioEvent::new(
                &clip(1.0, 0.0, None),
                &region(false, &[], 0.0),
                empty,
                Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR },
                (1.0, 1.0)
            )
            .is_none()
        );
    }
}
