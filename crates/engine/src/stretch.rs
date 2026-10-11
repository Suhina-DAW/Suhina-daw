//! Real-time time-stretching of warped audio clips (ADR 0003).
//!
//! Each warped clip placement gets a [`StretchVoice`]: a timestretch engine built when the
//! renderer is built, fed from the decoded file in memory and run inside `process`. Its playlist
//! says which part of the file plays when and how fast (warp marker segments, loop jumps), so the
//! voice only copies source frames, schedules rate changes at exact frames and runs the stretcher.
//! Nothing here allocates once the voice exists (AGENTS.md rule 3).
//!
//! The stretcher runs at the file's sample rate, so pitch is kept against the file's own pitch;
//! its output is then read at the project's sample rate with linear interpolation.

use std::sync::Arc;

use loopwerk_model::{AudioRegion, Clip, StretchMode};
use timestretch::PreAnalysisArtifact;
use timestretch::engine::{
    Engine, EngineConfig, EngineController, EngineProcessor, EngineProfile, MAX_PENDING_RETARGETS, SourceProducer,
};

use crate::audio::AudioData;

/// A file's pre-analysis (onsets, beat grid), computed once per file when a project loads and
/// shared by every voice that plays it.
#[derive(Debug, Clone)]
pub struct AudioAnalysis(Arc<PreAnalysisArtifact>);

/// Analyses a decoded file for the stretcher. Takes about 0.5 s per 80 s of audio; call it off
/// the audio thread.
pub fn analyze_audio(data: &AudioData) -> AudioAnalysis {
    let channels = usize::from(data.channels.max(1));
    let mid = if channels == 1 { data.samples.clone() } else { timestretch::downmix_to_mid(&data.samples, channels) };
    AudioAnalysis(Arc::new(timestretch::analyze(&mid, data.sample_rate)))
}

/// Stretcher frames processed per call; also the stretcher's block size.
const CHUNK_FRAMES: usize = 1024;
/// Speeds the keylock profile keeps the pitch for (timestretch's DJ range).
const KEYLOCK_RATES: std::ops::RangeInclusive<f64> = 0.8..=1.2;
/// Speeds this close to 1 play without a stretcher.
const UNITY_TOLERANCE: f64 = 1e-4;
/// Speeds the full-range profile handles.
const WIDE_RATES: std::ops::RangeInclusive<f64> = 0.25..=2.0;
/// Most playlist pieces per clip, so a malformed clip cannot exhaust memory.
const MAX_PIECES: usize = 100_000;

/// A stretch of the playlist: from `out_start` seconds after the clip starts, play file frames
/// `src_start..src_end` at `rate` times their own speed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    out_start: f64,
    src_start: f64,
    src_end: f64,
    rate: f64,
}

/// Works out which part of the file plays when, from the clip's offset, loop and length and the
/// region's warp markers at `tempo_bpm`. `None` when the clip cannot be stretched (fewer than two
/// usable warp markers, or a speed outside what the stretcher handles).
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
        pieces.push(Piece { out_start: played * seconds_per_beat, src_start, src_end, rate });
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
    controller: EngineController,
    processor: EngineProcessor,
    source: SourceProducer,
    /// Feed cursor: current piece and the next file frame to push.
    feed_piece: usize,
    feed_frame: usize,
    /// Rate changes as (stretcher output frame, rate), and the next one to schedule.
    retargets: Vec<(u64, f64)>,
    next_retarget: usize,
    /// Stretcher frames to discard when the voice starts (pipeline delay).
    latency: usize,
    started: bool,
    /// Stretched audio at the file's rate, waiting to be read; `fifo_read` is the next unread
    /// frame and `fifo_position` the fractional read position within it.
    fifo: Vec<f32>,
    fifo_len: usize,
    fifo_read: usize,
    fifo_position: f64,
    /// File frames per project frame.
    step: f64,
    chunk: Vec<f32>,
    zeros: Vec<f32>,
}

impl StretchVoice {
    /// Builds the voice for one clip placement. Allocates; call it off the audio thread.
    pub(crate) fn new(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        analysis: Option<&AudioAnalysis>,
        tempo_bpm: f64,
        sample_rate: u32,
    ) -> Option<Self> {
        let channels = usize::from(data.channels);
        if !(1..=8).contains(&channels) || data.frames() == 0 || !(8_000..=192_000).contains(&data.sample_rate) {
            return None;
        }
        let (pieces, max_rate) = playlist(clip, region, &data, tempo_bpm)?;
        let rates_ok = |range: &std::ops::RangeInclusive<f64>| pieces.iter().all(|p| range.contains(&p.rate));
        // Audio that already plays at its own speed needs no stretcher: the caller reads it directly.
        if rates_ok(&(1.0 - UNITY_TOLERANCE..=1.0 + UNITY_TOLERANCE)) {
            return None;
        }
        // Within ±20 % the time-domain keylock keeps the pitch (bass included beyond ~2 %) at a low,
        // even cost; the full-range phase vocoder is for larger changes (ADR 0003).
        let profile = match region.stretch {
            StretchMode::Repitch => return None,
            _ if rates_ok(&KEYLOCK_RATES) => EngineProfile::Keylock,
            _ if rates_ok(&WIDE_RATES) => EngineProfile::WideKeylock,
            _ => return None,
        };
        let first_rate = pieces.first().map_or(1.0, |p| p.rate);
        let handles = Engine::build(EngineConfig {
            sample_rate: data.sample_rate,
            channels,
            profile,
            initial_tempo_rate: first_rate,
            max_block_frames: CHUNK_FRAMES,
            source_capacity_frames: (CHUNK_FRAMES as f64 * max_rate.max(1.0) * 8.0) as usize + 8_192,
            pre_analysis: analysis.map(|a| a.0.clone()),
        })
        .ok()?;
        let latency = handles.processor.pipeline_latency_frames();
        let file_rate = f64::from(data.sample_rate);
        let retargets = pieces
            .windows(2)
            .filter_map(|w| match w {
                [a, b] if (a.rate - b.rate).abs() > 1e-12 => {
                    Some(((b.out_start * file_rate).round() as u64 + latency as u64, b.rate))
                }
                _ => None,
            })
            .collect();
        let step = file_rate / f64::from(sample_rate.max(1));
        // Room for one project block of up to CHUNK_FRAMES frames at any step, plus a chunk.
        let fifo_frames = (CHUNK_FRAMES as f64 * step.max(1.0)) as usize + 2 * CHUNK_FRAMES + 4;
        let mut voice = Self {
            feed_frame: pieces.first().map_or(0, |p| p.src_start.max(0.0) as usize),
            data,
            pieces,
            controller: handles.controller,
            processor: handles.processor,
            source: handles.source,
            feed_piece: 0,
            retargets,
            next_retarget: 0,
            latency,
            started: false,
            fifo: vec![0.0; fifo_frames * channels],
            fifo_len: 0,
            fifo_read: 0,
            fifo_position: 0.0,
            step,
            chunk: vec![0.0; CHUNK_FRAMES * channels],
            zeros: vec![0.0; 256 * channels],
        };
        voice.source.set_track_position(voice.feed_frame as u64);
        Some(voice)
    }

    fn channels(&self) -> usize {
        usize::from(self.data.channels)
    }

    /// Pushes playlist audio into the stretcher's ring until it is full or the playlist ends.
    fn feed(&mut self) {
        let channels = self.channels();
        let file_frames = self.data.frames();
        while self.source.free_frames() > 0 {
            let Some(piece) = self.pieces.get(self.feed_piece) else { return };
            let end = piece.src_end.max(0.0) as usize;
            if self.feed_frame >= end {
                self.feed_piece += 1;
                if let Some(next) = self.pieces.get(self.feed_piece) {
                    let start = next.src_start.max(0.0) as usize;
                    if start != self.feed_frame {
                        // A loop jump: re-anchor so the analysis stays aligned.
                        self.feed_frame = start;
                        self.source.set_track_position(start as u64);
                    }
                }
                continue;
            }
            let until = end.min(self.feed_frame + 8_192);
            let pushed = if self.feed_frame >= file_frames {
                // Past the end of the file: silence keeps the timeline.
                let frames = (until - self.feed_frame).min(self.zeros.len() / channels);
                self.source.push(self.zeros.get(..frames * channels).unwrap_or_default())
            } else {
                let until = until.min(file_frames);
                self.source
                    .push(self.data.samples.get(self.feed_frame * channels..until * channels).unwrap_or_default())
            };
            if pushed == 0 {
                return;
            }
            self.feed_frame += pushed;
        }
    }

    /// Schedules the coming rate changes, a few at a time (the stretcher queues at most 8).
    fn schedule(&mut self) {
        let delivered = self.controller.delivered_frames();
        let horizon = delivered + 4 * CHUNK_FRAMES as u64;
        while let Some(&(at, rate)) = self.retargets.get(self.next_retarget) {
            let pending = self
                .retargets
                .get(..self.next_retarget)
                .map_or(0, |r| r.iter().filter(|(a, _)| *a > delivered).count());
            if at > horizon || pending >= MAX_PENDING_RETARGETS / 2 {
                return;
            }
            self.controller.set_tempo_rate_at(rate, at);
            self.next_retarget += 1;
        }
    }

    /// Runs the stretcher for one chunk and appends its output to the FIFO.
    fn run_chunk(&mut self) {
        let channels = self.channels();
        // Make room by dropping frames that have been read.
        if self.fifo_read > 0 {
            let read = self.fifo_read * channels;
            let len = self.fifo_len * channels;
            self.fifo.copy_within(read..len, 0);
            self.fifo_len -= self.fifo_read;
            self.fifo_read = 0;
        }
        let free = self.fifo.len() / channels - self.fifo_len;
        let frames = CHUNK_FRAMES.min(free);
        if frames == 0 {
            return;
        }
        self.feed();
        self.schedule();
        let Some(chunk) = self.chunk.get_mut(..frames * channels) else { return };
        self.processor.process(chunk);
        let at = self.fifo_len * channels;
        if let Some(dst) = self.fifo.get_mut(at..at + frames * channels) {
            dst.copy_from_slice(chunk);
            self.fifo_len += frames;
        }
    }

    /// Renders the next `out.len() / 2` project frames as interleaved stereo into `out`.
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        let channels = self.channels();
        if !self.started {
            self.started = true;
            // Discard the pipeline delay so the first frame we play is the clip's first frame.
            let mut left = self.latency;
            while left > 0 {
                let frames = left.min(CHUNK_FRAMES);
                self.feed();
                self.schedule();
                if let Some(chunk) = self.chunk.get_mut(..frames * channels) {
                    self.processor.process(chunk);
                }
                left -= frames;
            }
        }
        for frame in out.as_chunks_mut::<2>().0 {
            // Linear interpolation needs this frame and the next.
            while self.fifo_len < self.fifo_read + 2 {
                let before = self.fifo_len;
                self.run_chunk();
                if self.fifo_len == before {
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
            extensions: Extensions::default(),
        }
    }

    #[test]
    fn playlist_follows_markers_and_loops() {
        let data = sine(440.0, 4.0, 1000);
        // Beats 0–4 = 0–2 s, beats 4–6 = 2–4 s; loop beats 2–6; play 8 beats from beat 3.
        let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Smooth);
        let (pieces, _) = playlist(&clip(8.0, 3.0, Some((2.0, 6.0))), &r, &data, 120.0).unwrap();
        let summary: Vec<(f64, f64, f64, f64)> =
            pieces.iter().map(|p| (p.out_start, p.src_start, p.src_end, p.rate)).collect();
        assert_eq!(
            summary,
            [
                (0.0, 1500.0, 2000.0, 1.0), // beat 3–4
                (0.5, 2000.0, 4000.0, 2.0), // beat 4–6, twice as fast
                (1.5, 1000.0, 2000.0, 1.0), // loop back to beat 2–4
                (2.5, 2000.0, 4000.0, 2.0), // beat 4–6
                (3.5, 1000.0, 1500.0, 1.0), // beat 2–3, clip ends
            ]
        );
    }

    #[test]
    fn unusable_clips_get_no_voice() {
        let data = sine(440.0, 1.0, 48_000);
        let one_marker = region(&[(0.0, 0.0)], StretchMode::Smooth);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &one_marker, data.clone(), None, 120.0, 48_000).is_none());
        let unity = region(&[(0.0, 0.0), (2.0, 1.0)], StretchMode::Smooth);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &unity, data.clone(), None, 120.0, 48_000).is_none());
        let repitch = region(&[(0.0, 0.0), (2.0, 0.9)], StretchMode::Repitch);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &repitch, data.clone(), None, 120.0, 48_000).is_none());
        // 1 beat holding 0.02 s at 120 bpm: 0.04× speed, outside the stretcher's range.
        let extreme = region(&[(0.0, 0.0), (1.0, 0.02)], StretchMode::Smooth);
        assert!(StretchVoice::new(&clip(1.0, 0.0, None), &extreme, data, None, 120.0, 48_000).is_none());
    }
}
