//! Real-time time-stretching of warped audio clips (ADR 0003).
//!
//! Each warped clip placement gets a [`StretchVoice`]: a [`loopwerk_stretch::Stretch`] built with
//! the renderer and fed from the decoded file in memory. Its playlist says which part of the file
//! plays when and how fast (warp marker segments, loop jumps). The stretcher reads the playlist
//! as one continuous input stream; each call covers one stretch of constant speed, and the speed
//! is the ratio of the input it is given to the output it makes. Nothing here allocates once the
//! voice exists (AGENTS.md rule 3).
//!
//! The stretcher runs at the file's sample rate, so pitch is kept against the file's own pitch;
//! its output is then read at the project's sample rate with linear interpolation. Transpose is
//! the stretcher's own pitch shift.

use std::sync::Arc;

use loopwerk_model::{AudioRegion, Clip, StretchMode};
use loopwerk_stretch::Stretch;

use crate::audio::{AudioData, STRETCH_SAMPLE_RATES};

/// Stretcher frames made per call.
const CHUNK_FRAMES: usize = 1024;
/// Speeds this close to 1 play without a stretcher (when not transposed).
const UNITY_TOLERANCE: f64 = 1e-4;
/// Speeds the stretcher is given; a clip with a segment outside them is re-pitched.
const RATES: std::ops::RangeInclusive<f64> = 0.25..=4.0;
/// Most playlist pieces per clip, so a malformed clip cannot exhaust memory.
const MAX_PIECES: usize = 100_000;

/// A stretch of the playlist: from `out_start` seconds after the clip starts, play file frames
/// `src_start..src_end` at `rate` times their own speed. `stream_start` is where the piece begins
/// in the stretcher's input stream (all pieces' file frames, one after another).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    out_start: f64,
    src_start: f64,
    src_end: f64,
    rate: f64,
    stream_start: f64,
}

/// Works out which part of the file plays when, from the clip's offset, loop and length and the
/// region's warp markers at `tempo_bpm`. `None` when the clip cannot be stretched (fewer than two
/// usable warp markers, or a speed that is not a finite positive number).
fn playlist(clip: &Clip, region: &AudioRegion, data: &AudioData, tempo_bpm: f64) -> Option<(Vec<Piece>, f64)> {
    let mut markers: Vec<(f64, f64)> = region
        .warp_markers
        .iter()
        .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
        .map(|m| (m.time, m.audio_time))
        .collect();
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    markers.dedup_by(|a, b| a.0 == b.0);
    if markers.len() < 2 || markers.windows(2).any(|w| matches!(w, [a, b] if b.1 <= a.1)) {
        return None;
    }
    if !(tempo_bpm.is_finite() && tempo_bpm > 0.0 && clip.length.is_finite() && clip.length > 0.0) {
        return None;
    }
    let seconds_per_beat = 60.0 / tempo_bpm;
    let file_rate = f64::from(data.sample_rate);
    let loop_range = clip.loop_range.filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start);

    let mut pieces = Vec::new();
    let mut beat = clip.offset;
    let mut played = 0.0;
    let mut stream = 0.0;
    let mut max_rate: f64 = 0.0;
    while played < clip.length - 1e-9 && pieces.len() < MAX_PIECES {
        let next_marker = markers.iter().map(|m| m.0).find(|b| *b > beat + 1e-9);
        let loop_end = loop_range.map(|r| r.end).filter(|e| *e > beat + 1e-9);
        let clip_end = beat + (clip.length - played);
        let next = [next_marker, loop_end, Some(clip_end)].into_iter().flatten().fold(f64::INFINITY, f64::min);
        let src_start = seconds_at(&markers, beat) * file_rate;
        let src_end = seconds_at(&markers, next) * file_rate;
        let out_seconds = (next - beat) * seconds_per_beat;
        let rate = (src_end - src_start) / file_rate / out_seconds;
        if !(rate.is_finite() && rate > 0.0) {
            return None;
        }
        max_rate = max_rate.max(rate).max(1.0 / rate);
        pieces.push(Piece { out_start: played * seconds_per_beat, src_start, src_end, rate, stream_start: stream });
        stream += src_end - src_start;
        played += next - beat;
        beat = next;
        if let Some(r) = loop_range
            && beat >= r.end - 1e-9
        {
            beat = r.start;
        }
    }
    Some((pieces, max_rate))
}

/// File seconds at a clip beat, extending the first and last marker segments.
fn seconds_at(markers: &[(f64, f64)], beat: f64) -> f64 {
    let i = markers.iter().position(|m| m.0 > beat).unwrap_or(markers.len()).clamp(1, markers.len().max(2) - 1);
    let (Some(a), Some(b)) = (markers.get(i - 1), markers.get(i)) else { return 0.0 };
    if b.0 <= a.0 {
        return a.1;
    }
    a.1 + (beat - a.0) * (b.1 - a.1) / (b.0 - a.0)
}

/// One warped clip placement being stretched in real time.
pub(crate) struct StretchVoice {
    data: Arc<AudioData>,
    pieces: Vec<Piece>,
    stretch: Stretch,
    channels: usize,
    /// Stretcher output frames per second of the clip (the file's sample rate).
    file_rate: f64,
    /// Input-stream frames given to the stretcher so far.
    fed: f64,
    /// Stretcher frames produced so far: the voice's own clock, at the file's sample rate.
    produced: u64,
    /// Stretched audio at the file's rate, waiting to be read; `fifo_read` is the next unread
    /// frame and `fifo_position` the fractional read position within it.
    fifo: Vec<f32>,
    fifo_len: usize,
    fifo_read: usize,
    fifo_position: f64,
    /// File frames per project frame.
    step: f64,
    input: Vec<f32>,
    chunk: Vec<f32>,
}

impl StretchVoice {
    /// Builds the voice for one clip placement, ready to play from the clip's start. Allocates;
    /// call it off the audio thread.
    pub(crate) fn new(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        tempo_bpm: f64,
        sample_rate: u32,
    ) -> Option<Self> {
        let channels = usize::from(data.channels);
        if !(1..=8).contains(&channels) || data.frames() == 0 || !STRETCH_SAMPLE_RATES.contains(&data.sample_rate) {
            return None;
        }
        if region.stretch == StretchMode::Repitch {
            return None;
        }
        let (pieces, max_rate) = playlist(clip, region, &data, tempo_bpm)?;
        let pitch = 2f64.powf(region.transpose / 12.0);
        let pitch = if pitch.is_finite() && pitch > 0.0 { pitch } else { 1.0 };
        // Audio that already plays at its own speed and pitch needs no stretcher: the caller reads
        // it directly.
        if pitch == 1.0 && pieces.iter().all(|p| (p.rate - 1.0).abs() <= UNITY_TOLERANCE) {
            return None;
        }
        if !pieces.iter().all(|p| RATES.contains(&p.rate)) {
            return None;
        }
        let file_rate = f64::from(data.sample_rate);
        let mut stretch = Stretch::new(channels, file_rate as f32);
        if pitch != 1.0 {
            stretch.set_transpose_factor(pitch as f32, 0.0);
        }
        let step = file_rate / f64::from(sample_rate.max(1));
        // Input for one chunk at the fastest speed, or for a seek (which wants up to one block
        // plus the output latency at that speed).
        let input_frames = (CHUNK_FRAMES as f64 * max_rate).ceil() as usize
            + stretch.block_samples() * 2
            + (max_rate * stretch.output_latency() as f64).ceil() as usize
            + 2;
        // Room for one project block of up to CHUNK_FRAMES frames at any step, plus a chunk.
        let fifo_frames = (CHUNK_FRAMES as f64 * step.max(1.0)) as usize + 2 * CHUNK_FRAMES + 4;
        let mut voice = Self {
            data,
            pieces,
            stretch,
            channels,
            file_rate,
            fed: 0.0,
            produced: 0,
            fifo: vec![0.0; fifo_frames * channels],
            fifo_len: 0,
            fifo_read: 0,
            fifo_position: 0.0,
            step,
            input: vec![0.0; input_frames * channels],
            chunk: vec![0.0; CHUNK_FRAMES * channels],
        };
        voice.seek(0);
        Some(voice)
    }

    /// Rewinds the voice to the clip's first frame, for a transport restart or a relaunch.
    /// Never allocates.
    pub(crate) fn restart(&mut self) {
        self.seek(0);
    }

    /// Moves the voice so that its next frame is `frames` project frames after the clip's start.
    /// Never allocates; runs the stretcher's start-up on the calling thread.
    pub(crate) fn seek(&mut self, frames: u64) {
        let at = (frames as f64 * self.step).round() as u64;
        let index = self.piece_at(at);
        let Some(piece) = self.pieces.get(index).copied() else { return };
        let into_piece = (at as f64 - piece.out_start * self.file_rate).max(0.0);
        let stream = (piece.stream_start + into_piece * piece.rate).floor();
        let length = self.stretch.output_seek_length(piece.rate as f32);
        let channels = self.channels;
        let mut input = std::mem::take(&mut self.input);
        let length = length.min(input.len() / channels);
        if let Some(buffer) = input.get_mut(..length * channels) {
            self.read_stream(stream, buffer);
            self.stretch.output_seek(buffer);
        }
        self.input = input;
        self.fed = stream + length as f64;
        self.produced = at;
        self.fifo_len = 0;
        self.fifo_read = 0;
        self.fifo_position = 0.0;
    }

    /// Where in the input stream voice frame `at` plays.
    fn stream_at(&self, at: u64) -> f64 {
        let Some(piece) = self.pieces.get(self.piece_at(at)) else { return 0.0 };
        piece.stream_start + (at as f64 - piece.out_start * self.file_rate).max(0.0) * piece.rate
    }

    /// How far the input must run ahead of what is heard, at `rate`: the stretcher analyses a block
    /// `input_latency` frames behind its input and is heard `output_latency` frames after that.
    fn lead(&self, rate: f64) -> f64 {
        self.stretch.input_latency() as f64 + rate * self.stretch.output_latency() as f64
    }

    /// The piece playing at voice frame `at` (the last one past the end).
    fn piece_at(&self, at: u64) -> usize {
        let file_rate = self.file_rate;
        self.pieces.iter().rposition(|p| (p.out_start * file_rate).round() as u64 <= at).unwrap_or(0)
    }

    /// Fills `out` (interleaved) with the input stream from frame `from` on. Past the playlist
    /// the file carries on; past the file there is silence.
    fn read_stream(&self, from: f64, out: &mut [f32]) {
        let channels = self.channels;
        let file_frames = self.data.frames();
        let mut index = self.pieces.partition_point(|p| p.stream_start <= from).saturating_sub(1);
        for (i, frame) in out.chunks_exact_mut(channels).enumerate() {
            let position = from + i as f64;
            while let Some(next) = self.pieces.get(index + 1)
                && next.stream_start <= position
            {
                index += 1;
            }
            let file_frame = self.pieces.get(index).map_or(-1.0, |p| p.src_start + (position - p.stream_start));
            let file_frame = file_frame.floor();
            for (c, s) in frame.iter_mut().enumerate() {
                *s = if file_frame >= 0.0 && (file_frame as usize) < file_frames {
                    self.data.samples.get(file_frame as usize * channels + c).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
            }
        }
    }

    /// Runs the stretcher for up to one chunk, at one speed, and appends the output to the FIFO.
    fn run_chunk(&mut self) {
        let channels = self.channels;
        // Make room by dropping frames that have been read.
        if self.fifo_read > 0 {
            let read = self.fifo_read.min(self.fifo_len);
            if let Some(unread) = self.fifo.get(read * channels..self.fifo_len * channels) {
                let unread = unread.len();
                self.fifo.copy_within(read * channels..read * channels + unread, 0);
            }
            self.fifo_len -= read;
            self.fifo_read = 0;
        }
        let free = self.fifo.len() / channels - self.fifo_len;
        let mut frames = CHUNK_FRAMES.min(free) as u64;
        if frames == 0 {
            return;
        }
        // One speed per call: stop at the next piece.
        let index = self.piece_at(self.produced);
        if let Some(next) = self.pieces.get(index + 1) {
            let boundary = (next.out_start * self.file_rate).round() as u64;
            if boundary > self.produced {
                frames = frames.min(boundary - self.produced);
            }
        }
        let rate = self.pieces.get(index).map_or(1.0, |p| p.rate);
        // Feed up to where the input should be once these frames are out. The lead depends on the
        // speed, so a speed change is evened out over the next call instead of shifting the
        // timing for good.
        let fed = (self.stream_at(self.produced + frames) + self.lead(rate)).max(self.fed);
        let input_frames = ((fed.floor() - self.fed.floor()).max(0.0) as usize).min(self.input.len() / channels);
        let frames = frames as usize;
        let mut input = std::mem::take(&mut self.input);
        let mut chunk = std::mem::take(&mut self.chunk);
        if let (Some(input_buffer), Some(output)) =
            (input.get_mut(..input_frames * channels), chunk.get_mut(..frames * channels))
        {
            self.read_stream(self.fed.floor(), input_buffer);
            self.stretch.process(input_buffer, output);
        }
        self.input = input;
        self.chunk = chunk;
        self.fed = fed;
        let at = self.fifo_len * channels;
        if let (Some(dst), Some(src)) =
            (self.fifo.get_mut(at..at + frames * channels), self.chunk.get(..frames * channels))
        {
            dst.copy_from_slice(src);
            self.fifo_len += frames;
            self.produced += frames as u64;
        }
    }

    /// Renders the next `out.len() / 2` project frames as interleaved stereo into `out`.
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        let channels = self.channels;
        for frame in out.as_chunks_mut::<2>().0 {
            // Linear interpolation needs this frame and the next.
            while self.fifo_len < self.fifo_read + 2 {
                // Progress is counted in frames produced: compacting the FIFO can leave its length
                // unchanged even when a chunk was added.
                let before = self.produced;
                self.run_chunk();
                if self.produced == before {
                    break;
                }
            }
            let at = |i: usize, c: usize| self.fifo.get(i * channels + c.min(channels - 1)).copied().unwrap_or(0.0);
            let t = self.fifo_position as f32;
            let i = self.fifo_read;
            frame[0] = at(i, 0) + (at(i + 1, 0) - at(i, 0)) * t;
            frame[1] = at(i, 1) + (at(i + 1, 1) - at(i, 1)) * t;
            self.fifo_position += self.step;
            let whole = self.fifo_position.floor();
            self.fifo_read += whole as usize;
            self.fifo_position -= whole;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loopwerk_model::*;

    fn sine(hz: f64, seconds: f64, sample_rate: u32) -> Arc<AudioData> {
        let frames = (seconds * f64::from(sample_rate)) as usize;
        let samples = (0..frames)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(sample_rate)).sin() as f32 * 0.5)
            .collect();
        Arc::new(AudioData { sample_rate, channels: 1, samples })
    }

    fn region(markers: &[(f64, f64)], mode: StretchMode) -> AudioRegion {
        AudioRegion {
            file: "sine.wav".into(),
            absolute_file: None,
            sample_rate: None,
            channels: None,
            duration_seconds: None,
            warped: true,
            stretch: mode,
            warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
            gain: 1.0,
            transpose: 0.0,
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

    #[test]
    fn playlist_follows_markers_and_loops() {
        let data = sine(440.0, 4.0, 1000);
        // Beats 0–4 = 0–2 s, beats 4–6 = 2–4 s; loop beats 2–6; play 8 beats from beat 3.
        let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Smooth);
        let (pieces, _) = playlist(&clip(8.0, 3.0, Some((2.0, 6.0))), &r, &data, 120.0).unwrap();
        let summary: Vec<(f64, f64, f64, f64, f64)> =
            pieces.iter().map(|p| (p.out_start, p.src_start, p.src_end, p.rate, p.stream_start)).collect();
        assert_eq!(
            summary,
            [
                (0.0, 1500.0, 2000.0, 1.0, 0.0),    // beat 3–4
                (0.5, 2000.0, 4000.0, 2.0, 500.0),  // beat 4–6, twice as fast
                (1.5, 1000.0, 2000.0, 1.0, 2500.0), // loop back to beat 2–4
                (2.5, 2000.0, 4000.0, 2.0, 3500.0), // beat 4–6
                (3.5, 1000.0, 1500.0, 1.0, 5500.0), // beat 2–3, clip ends
            ]
        );
    }

    /// Clicks every `every` seconds: a 2 ms burst at full scale, silence between.
    fn clicks(every: f64, seconds: f64, sample_rate: u32) -> Arc<AudioData> {
        let frames = (seconds * f64::from(sample_rate)) as usize;
        let period = (every * f64::from(sample_rate)) as usize;
        let burst = (0.002 * f64::from(sample_rate)) as usize;
        let samples = (0..frames).map(|i| if i % period < burst { 0.9 } else { 0.0 }).collect();
        Arc::new(AudioData { sample_rate, channels: 1, samples })
    }

    /// Onset times (seconds) in interleaved stereo: where the level first rises above a threshold
    /// after at least 50 ms below it.
    fn onsets(stereo: &[f32], sample_rate: u32) -> Vec<f64> {
        let quiet_needed = (0.05 * f64::from(sample_rate)) as usize;
        let mut quiet = quiet_needed;
        let mut found = Vec::new();
        for (i, frame) in stereo.as_chunks::<2>().0.iter().enumerate() {
            if frame[0].abs() > 0.2 {
                if quiet >= quiet_needed {
                    found.push(i as f64 / f64::from(sample_rate));
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        found
    }

    /// Renders a warped click clip (transposed by `transpose`, from `seek` seconds into it) and
    /// returns how far each click lands from where the warp markers put it, in milliseconds.
    fn click_errors(markers: &[(f64, f64)], length_beats: f64, seek: f64, transpose: f64) -> Vec<f64> {
        let sr = 48_000;
        let data = clicks(0.5, 40.0, sr);
        let r = AudioRegion { transpose, ..region(markers, StretchMode::Smooth) };
        let c = clip(length_beats, 0.0, None);
        let mut voice = StretchVoice::new(&c, &r, data, 120.0, sr).unwrap();
        let skipped = (seek * f64::from(sr)) as usize;
        if skipped > 0 {
            voice.seek(skipped as u64);
        }
        let frames = (length_beats * 0.5 * f64::from(sr)) as usize - skipped;
        let mut out = vec![0.0; frames * 2];
        for block in out.chunks_mut(512 * 2) {
            voice.render(block);
        }
        // Where each click should play: file time → beat through the markers → seconds at 120 bpm.
        let beat_of = |seconds: f64| {
            let i = markers.iter().position(|m| m.1 > seconds).unwrap_or(markers.len()).clamp(1, markers.len() - 1);
            let (a, b) = (markers[i - 1], markers[i]);
            a.0 + (seconds - a.1) * (b.0 - a.0) / (b.1 - a.1)
        };
        onsets(&out, sr)
            .iter()
            .map(|&t| t + seek)
            .map(|t| {
                // The nearest click in the file to this onset.
                let expected = (0..80)
                    .map(|k| beat_of(k as f64 * 0.5) * 0.5)
                    .fold(f64::INFINITY, |best, e| if (e - t).abs() < (best - t).abs() { e } else { best });
                (t - expected) * 1000.0
            })
            .collect()
    }

    fn worst(errors: &[f64]) -> f64 {
        errors.iter().fold(0.0f64, |w, x| w.max(x.abs()))
    }

    const SEGMENTS: [(f64, f64); 6] = [(0.0, 0.0), (8.0, 3.9), (16.0, 8.1), (24.0, 11.9), (32.0, 16.2), (48.0, 23.8)];

    #[test]
    fn transients_stay_on_the_grid() {
        for (markers, limit) in [
            (vec![(0.0, 0.0), (64.0, 30.4)], 3.0), // 0.95x
            (vec![(0.0, 0.0), (64.0, 33.6)], 3.0), // 1.05x
            (vec![(0.0, 0.0), (64.0, 22.4)], 4.0), // 0.7x
            (vec![(0.0, 0.0), (64.0, 48.0)], 4.0), // 1.5x
            (SEGMENTS.to_vec(), 3.0),              // changing speed
        ] {
            let errors = click_errors(&markers, 46.0, 0.0, 0.0);
            assert!(errors.len() >= 30, "{markers:?}: {} clicks found", errors.len());
            assert!(worst(&errors) < limit, "{markers:?}: {errors:?}");
        }
    }

    #[test]
    fn a_voice_can_start_anywhere_in_its_clip() {
        // Seek into the middle of a beat, of a speed segment, and just past a segment change.
        for seek in [3.3, 9.87, 16.01] {
            let errors = click_errors(&SEGMENTS, 46.0, seek, 0.0);
            assert!(errors.len() >= 10, "seek {seek}: {} clicks found", errors.len());
            assert!(worst(&errors) < 4.0, "seek {seek}: {errors:?}");
        }
    }

    #[test]
    fn transposed_clips_keep_transients_on_the_grid() {
        for transpose in [-7.0, -3.0, 2.0, 7.0] {
            let errors = click_errors(&[(0.0, 0.0), (64.0, 30.4)], 46.0, 0.0, transpose);
            assert!(errors.len() >= 40, "{transpose} st: {} clicks found", errors.len());
            assert!(worst(&errors) < 4.0, "{transpose} st: {errors:?}");
        }
    }

    /// Renders a warped sine clip and returns (frequency in Hz, frames with sound) at 48 kHz.
    fn transposed(hz: f64, markers: &[(f64, f64)], beats: f64, transpose: f64) -> (f64, usize) {
        let sr = 48_000;
        let data = sine(hz, 20.0, 44_100);
        let r = AudioRegion { transpose, ..region(markers, StretchMode::Smooth) };
        let mut voice = StretchVoice::new(&clip(beats, 0.0, None), &r, data, 120.0, sr).unwrap();
        let frames = (beats * 0.5 * f64::from(sr)) as usize;
        let mut out = vec![0.0; frames * 2];
        for block in out.chunks_mut(512 * 2) {
            voice.render(block);
        }
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let middle = &left[left.len() / 4..left.len() * 3 / 4];
        let ups = middle.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let sounding = left.iter().rposition(|s| s.abs() > 0.05).map_or(0, |i| i + 1);
        (ups as f64 / (middle.len() as f64 / f64::from(sr)), sounding)
    }

    #[test]
    fn transpose_shifts_pitch_but_not_time_on_warped_clips() {
        // 8 beats at 120 bpm = 4 s, the file playing at its own speed, then at 0.9x.
        for (markers, transpose, expected) in [
            (vec![(0.0, 0.0), (8.0, 4.0)], 12.0, 880.0),
            (vec![(0.0, 0.0), (8.0, 4.0)], -5.0, 440.0 * 2f64.powf(-5.0 / 12.0)),
            (vec![(0.0, 0.0), (8.0, 3.6)], 7.0, 440.0 * 2f64.powf(7.0 / 12.0)),
            (vec![(0.0, 0.0), (8.0, 3.6)], 0.0, 440.0),
        ] {
            let (hz, sounding) = transposed(440.0, &markers, 8.0, transpose);
            assert!((hz - expected).abs() < expected * 0.01, "{transpose} st: {hz:.1} Hz, expected {expected:.1}");
            // The clip still fills its 4 s.
            assert!(sounding > 4 * 48_000 - 2_000, "{transpose} st: sound ends at frame {sounding}");
        }
    }

    #[test]
    fn plays_through_at_any_device_rate_and_block_size() {
        // A 48 kHz file on 44.1 kHz and 96 kHz devices, in odd block sizes.
        let data = clicks(0.25, 40.0, 48_000);
        for markers in [vec![(0.0, 0.0), (64.0, 30.4)], vec![(0.0, 0.0), (64.0, 45.0)]] {
            let r = region(&markers, StretchMode::Smooth);
            for (device_rate, block) in [(44_100, 441), (44_100, 448), (96_000, 37), (22_050, 1000)] {
                let mut voice =
                    StretchVoice::new(&clip(60.0, 0.0, None), &r, data.clone(), 120.0, device_rate).unwrap();
                let mut out = vec![0.0; block * 2];
                for _ in 0..(30 * device_rate as usize / block) {
                    voice.render(&mut out);
                }
            }
        }
    }

    #[test]
    fn unusable_clips_get_no_voice() {
        let data = sine(440.0, 1.0, 48_000);
        let one_marker = region(&[(0.0, 0.0)], StretchMode::Smooth);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &one_marker, data.clone(), 120.0, 48_000).is_none());
        let unity = region(&[(0.0, 0.0), (2.0, 1.0)], StretchMode::Smooth);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &unity, data.clone(), 120.0, 48_000).is_none());
        let repitch = region(&[(0.0, 0.0), (2.0, 0.9)], StretchMode::Repitch);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &repitch, data.clone(), 120.0, 48_000).is_none());
        // 1 beat holding 0.02 s at 120 bpm: 0.04x speed, too slow to stretch.
        let extreme = region(&[(0.0, 0.0), (1.0, 0.02)], StretchMode::Smooth);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &extreme, data, 120.0, 48_000).is_none());
    }
}
