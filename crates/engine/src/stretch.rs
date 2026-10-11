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
//!
//! Every voice has two stretchers ("lanes"). Shortly before each strong onset the idle lane is
//! restarted at exactly the right source frame and the voice crossfades into it, so transients
//! land on the grid instead of drifting by up to half a stretcher grain (ADR 0003).

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
/// A stretcher keeps transients exactly in place only just after it starts; later they drift by up
/// to half its grain (several ms), so layered drums flam. A voice therefore restarts a fresh
/// stretcher this many frames before each strong onset and crossfades into it.
const ANCHOR_LEAD: u64 = 384;
/// Crossfade from the running stretcher into the fresh one, in stretcher frames.
const ANCHOR_FADE: u64 = 192;
/// Closest two restarts may be, in stretcher frames: keylock, and the phase vocoder, whose start-up
/// is several times longer.
const ANCHOR_MIN_GAP: u64 = 2_048;
const WIDE_MIN_GAP: u64 = 12_288;
/// Weakest onset (the analysis' normalised strength) that gets a restart.
const ANCHOR_MIN_STRENGTH: f32 = 0.1;
/// File frames the phase-vocoder profile is fed before a start point: two analysis windows plus
/// its rate settling.
const WIDE_PREROLL: usize = 4_096;
/// The phase vocoder's window delay, as measured by the click tests below.
const WIDE_DELAY_SECONDS: f64 = 0.031;

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

/// A running stretcher with its own feed and rate-change cursors. A voice has two: one plays, and
/// the other is restarted before each strong onset (see [`ANCHOR_LEAD`]).
struct Lane {
    controller: EngineController,
    processor: EngineProcessor,
    source: SourceProducer,
    /// Feed cursor: current piece and the next file frame to push.
    feed_piece: usize,
    feed_frame: usize,
    /// Frames of silence still to feed before the playlist, where the preroll would start before
    /// the file does.
    pad: usize,
    /// The first rate change this lane schedules, and the next one.
    first_retarget: usize,
    next_retarget: usize,
    /// Voice frame this lane started at, and the stretcher output frame that plays it.
    origin: u64,
    base: u64,
}

impl Lane {
    fn build(config: EngineConfig) -> Option<Self> {
        let handles = Engine::build(config).ok()?;
        Some(Self {
            controller: handles.controller,
            processor: handles.processor,
            source: handles.source,
            feed_piece: 0,
            feed_frame: 0,
            pad: 0,
            first_retarget: 0,
            next_retarget: 0,
            origin: 0,
            base: 0,
        })
    }

    /// Pushes playlist audio into the stretcher's ring until it is full or the playlist ends.
    fn feed(&mut self, pieces: &[Piece], data: &AudioData, zeros: &[f32]) {
        let channels = usize::from(data.channels);
        let file_frames = data.frames();
        while self.pad > 0 {
            let frames = self.pad.min(zeros.len() / channels);
            let pushed = self.source.push(zeros.get(..frames * channels).unwrap_or_default());
            if pushed == 0 {
                return;
            }
            self.pad -= pushed;
        }
        while self.source.free_frames() > 0 {
            let Some(piece) = pieces.get(self.feed_piece) else { return };
            let end = piece.src_end.max(0.0) as usize;
            if self.feed_frame >= end {
                self.feed_piece += 1;
                if let Some(next) = pieces.get(self.feed_piece) {
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
                let frames = (until - self.feed_frame).min(zeros.len() / channels);
                self.source.push(zeros.get(..frames * channels).unwrap_or_default())
            } else {
                let until = until.min(file_frames);
                self.source.push(data.samples.get(self.feed_frame * channels..until * channels).unwrap_or_default())
            };
            if pushed == 0 {
                return;
            }
            self.feed_frame += pushed;
        }
    }

    /// Schedules the coming rate changes, a few at a time (the stretcher queues at most 8).
    /// `retargets` are in voice frames; the stretcher counts from this lane's start.
    fn schedule(&mut self, retargets: &[(u64, f64)]) {
        let (origin, base) = (self.origin, self.base);
        let local = |at: u64| at.saturating_sub(origin) + base;
        let delivered = self.controller.delivered_frames();
        let horizon = delivered + 4 * CHUNK_FRAMES as u64;
        while let Some(&(at, rate)) = retargets.get(self.next_retarget) {
            let pending = retargets
                .get(self.first_retarget..self.next_retarget)
                .map_or(0, |r| r.iter().filter(|(a, _)| local(*a) > delivered).count());
            if local(at) > horizon || pending >= MAX_PENDING_RETARGETS / 2 {
                return;
            }
            self.controller.set_tempo_rate_at(rate, local(at));
            self.next_retarget += 1;
        }
    }

    /// Feeds, schedules and runs the stretcher for `out.len() / channels` frames.
    fn process(
        &mut self,
        out: &mut [f32],
        pieces: &[Piece],
        data: &AudioData,
        zeros: &[f32],
        retargets: &[(u64, f64)],
    ) {
        self.feed(pieces, data, zeros);
        self.schedule(retargets);
        self.processor.process(out);
    }
}

/// One warped clip placement being stretched in real time.
pub(crate) struct StretchVoice {
    data: Arc<AudioData>,
    pieces: Vec<Piece>,
    lanes: [Lane; 2],
    active: usize,
    /// Rate changes as (voice frame, rate).
    retargets: Vec<(u64, f64)>,
    /// Voice frames at which the idle lane restarts, and the next one.
    restarts: Vec<u64>,
    next_restart: usize,
    /// Crossfade frames still to go from the active lane into the other one.
    fade_left: u64,
    /// How a lane starts: run off this many frames of pipeline delay (keylock), or, when
    /// `preroll` is not zero, feed that many file frames ahead of the start point and run until
    /// the stretcher reports the start point playing (phase vocoder, which must also settle on
    /// its rate after a reset).
    latency: usize,
    preroll: usize,
    /// The idle lane is starting up for the next restart; what is left of a keylock start-up and
    /// the rate it starts at.
    priming: bool,
    prime_left: usize,
    prime_rate: f64,
    /// Voice frames before a restart over which its start-up is spread.
    prep_window: u64,
    /// Stretched frames produced so far: the voice's own clock, at the file's sample rate.
    produced: u64,
    /// Stretched audio at the file's rate, waiting to be read; `fifo_read` is the next unread
    /// frame and `fifo_position` the fractional read position within it.
    fifo: Vec<f32>,
    fifo_len: usize,
    fifo_read: usize,
    fifo_position: f64,
    /// File frames per project frame.
    step: f64,
    chunk: Vec<f32>,
    incoming: Vec<f32>,
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
        let config = || EngineConfig {
            sample_rate: data.sample_rate,
            channels,
            profile,
            initial_tempo_rate: pieces.first().map_or(1.0, |p| p.rate),
            max_block_frames: CHUNK_FRAMES,
            source_capacity_frames: (CHUNK_FRAMES as f64 * max_rate.max(1.0) * 8.0) as usize + 8_192,
            pre_analysis: analysis.map(|a| a.0.clone()),
        };
        let lanes = [Lane::build(config())?, Lane::build(config())?];
        // The keylock places transients exactly only right after a reset, so it starts with no
        // preroll; the phase vocoder needs a full analysis window and ~30 ms to settle on its rate.
        let latency = lanes[0].processor.pipeline_latency_frames();
        let preroll = if profile == EngineProfile::WideKeylock { WIDE_PREROLL } else { 0 };
        let file_rate = f64::from(data.sample_rate);
        let retargets = pieces
            .windows(2)
            .filter_map(|w| match w {
                [a, b] if (a.rate - b.rate).abs() > 1e-12 => Some(((b.out_start * file_rate).round() as u64, b.rate)),
                _ => None,
            })
            .collect();
        // The phase vocoder's start-up is several times longer, so it restarts less often.
        let min_gap = if preroll == 0 { ANCHOR_MIN_GAP } else { WIDE_MIN_GAP };
        let restarts = analysis.map(|a| restarts(&pieces, &a.0, file_rate, min_gap)).unwrap_or_default();
        let step = file_rate / f64::from(sample_rate.max(1));
        // Room for one project block of up to CHUNK_FRAMES frames at any step, plus a chunk.
        let fifo_frames = (CHUNK_FRAMES as f64 * step.max(1.0)) as usize + 2 * CHUNK_FRAMES + 4;
        let mut voice = Self {
            data,
            pieces,
            lanes,
            active: 0,
            retargets,
            restarts,
            next_restart: 0,
            fade_left: 0,
            latency,
            preroll,
            priming: false,
            prime_left: 0,
            prime_rate: 1.0,
            prep_window: min_gap - ANCHOR_FADE - 64,
            produced: 0,
            fifo: vec![0.0; fifo_frames * channels],
            fifo_len: 0,
            fifo_read: 0,
            fifo_position: 0.0,
            step,
            chunk: vec![0.0; CHUNK_FRAMES * channels],
            incoming: vec![0.0; CHUNK_FRAMES * channels],
            zeros: vec![0.0; 256 * channels],
        };
        // The first lane starts up here, off the audio thread, so clips that start together do
        // not all start up in one block.
        voice.begin_lane(0, 0);
        voice.prime(0, 0, 0);
        Some(voice)
    }

    fn channels(&self) -> usize {
        usize::from(self.data.channels)
    }

    /// Resets lane `index` and points it at voice frame `at`: it is fed from [`Self::preroll`] file
    /// frames before the point the playlist plays then, at that piece's rate. [`Self::prime`] then
    /// runs off its start-up so that its next output frame is `at`.
    fn begin_lane(&mut self, index: usize, at: u64) {
        let file_rate = f64::from(self.data.sample_rate);
        let piece_index = self.pieces.iter().rposition(|p| (p.out_start * file_rate).round() as u64 <= at).unwrap_or(0);
        let Some(piece) = self.pieces.get(piece_index).copied() else { return };
        let into_piece = at as f64 - piece.out_start * file_rate;
        let src = (piece.src_start + into_piece.max(0.0) * piece.rate).max(0.0).round() as usize;
        let first_retarget = self.retargets.partition_point(|r| r.0 <= at);
        let preroll = self.preroll;
        let from_file = preroll.min(src);
        self.prime_left = if preroll == 0 { self.latency } else { 0 };
        self.prime_rate = piece.rate;
        let Some(lane) = self.lanes.get_mut(index) else { return };
        lane.processor.reset();
        lane.controller.set_tempo_rate(piece.rate);
        lane.feed_piece = piece_index;
        lane.feed_frame = src - from_file;
        lane.pad = preroll - from_file;
        lane.source.set_track_position((src - from_file) as u64);
        lane.first_retarget = first_retarget;
        lane.next_retarget = first_retarget;
        lane.origin = at;
    }

    /// Runs off part of lane `index`'s start-up, discarding its output: the share `frames / until`
    /// of what is left, or all of it when `until` is 0. True once the lane is ready to play.
    ///
    /// The keylock's start-up is its pipeline delay. For the phase vocoder the stretcher's own map
    /// from output to source frames says when the start point plays; its window delay is not in
    /// that map, and measured, its output lands `WIDE_DELAY_SECONDS * (1 - 1 / rate)` late, so that
    /// much more (or less) is run off.
    fn prime(&mut self, index: usize, frames: u64, until: u64) -> bool {
        let channels = self.channels();
        let file_rate = f64::from(self.data.sample_rate);
        let (preroll, rate) = (self.preroll, self.prime_rate);
        let Self { lanes, pieces, data, zeros, incoming, prime_left, .. } = self;
        let Some(lane) = lanes.get_mut(index) else { return true };
        let mut budget = None;
        // Bounded, so a stretcher that never reports the start point cannot stall the audio thread.
        for _ in 0..64 {
            lane.feed(pieces, data, zeros);
            let ahead = if preroll == 0 {
                *prime_left
            } else {
                let window = WIDE_DELAY_SECONDS * file_rate * (1.0 - 1.0 / rate);
                let ahead = (preroll as f64 - lane.controller.source_position()) / rate + window;
                if ahead < 1.0 { 0 } else { ahead as usize }
            };
            if ahead == 0 {
                lane.base = lane.controller.delivered_frames();
                return true;
            }
            let left = *budget.get_or_insert(if until == 0 {
                usize::MAX
            } else {
                (ahead as u64 * frames).div_ceil(until) as usize
            });
            if left == 0 {
                return false;
            }
            let run = ahead.min(left).min(CHUNK_FRAMES);
            if let Some(scratch) = incoming.get_mut(..run * channels) {
                lane.processor.process(scratch);
            }
            *prime_left = prime_left.saturating_sub(run);
            budget = Some(left - run);
        }
        lane.base = lane.controller.delivered_frames();
        true
    }

    /// Runs the stretcher for up to one chunk, restarting and crossfading lanes at the restart
    /// points, and appends the output to the FIFO.
    fn run_chunk(&mut self) {
        let channels = self.channels();
        // Make room by dropping frames that have been read.
        if self.fifo_read > 0 {
            // Reading may have run past what was produced (a stall plays silence); drop that too.
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
        if self.fade_left == 0 {
            while self.restarts.get(self.next_restart).is_some_and(|r| *r < self.produced) {
                self.next_restart += 1;
                self.priming = false;
            }
            if let Some(at) = self.restarts.get(self.next_restart).copied() {
                let idle = 1 - self.active;
                let until = at - self.produced;
                // The idle lane's start-up is spread over the frames before its restart, so voices
                // whose restarts fall on the same beat do not all pay for them in one block.
                if !self.priming && until <= self.prep_window {
                    self.begin_lane(idle, at);
                    self.priming = true;
                }
                if !self.priming {
                    frames = frames.min(until - self.prep_window);
                } else if until == 0 {
                    self.prime(idle, 0, 0);
                    self.priming = false;
                    self.next_restart += 1;
                    self.fade_left = ANCHOR_FADE;
                } else {
                    frames = frames.min(until);
                    self.prime(idle, frames, until);
                }
            }
        }
        if self.fade_left > 0 {
            frames = frames.min(self.fade_left);
        }
        let frames = frames as usize;
        let Self { lanes, pieces, data, zeros, retargets, chunk, incoming, active, fade_left, .. } = self;
        let [first, second] = lanes;
        let (playing, next) = if *active == 0 { (first, second) } else { (second, first) };
        let (Some(out), Some(fresh)) = (chunk.get_mut(..frames * channels), incoming.get_mut(..frames * channels))
        else {
            return;
        };
        playing.process(out, pieces, data, zeros, retargets);
        if *fade_left > 0 {
            next.process(fresh, pieces, data, zeros, retargets);
            let done = ANCHOR_FADE - *fade_left;
            for (i, (o, f)) in out.chunks_exact_mut(channels).zip(fresh.chunks_exact(channels)).enumerate() {
                // Both lanes play the same audio, so a linear crossfade keeps the level.
                let gain = ((done + i as u64 + 1) as f32 / ANCHOR_FADE as f32).min(1.0);
                for (o, f) in o.iter_mut().zip(f) {
                    *o += (*f - *o) * gain;
                }
            }
            *fade_left -= frames as u64;
            if *fade_left == 0 {
                *active = 1 - *active;
            }
        }
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
        let channels = self.channels();
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

/// Voice frames at which a fresh lane takes over: [`ANCHOR_LEAD`] before each strong onset of the
/// file where the playlist plays it, at least `min_gap` apart.
fn restarts(pieces: &[Piece], analysis: &PreAnalysisArtifact, file_rate: f64, min_gap: u64) -> Vec<u64> {
    let mut onsets = Vec::new();
    for (i, &onset) in analysis.transient_onsets.iter().enumerate() {
        if analysis.transient_strengths.get(i).copied().unwrap_or(1.0) < ANCHOR_MIN_STRENGTH {
            continue;
        }
        let onset = onset as f64;
        for piece in pieces.iter().filter(|p| p.src_start <= onset && onset < p.src_end) {
            if onsets.len() < MAX_PIECES {
                onsets.push((piece.out_start * file_rate + (onset - piece.src_start) / piece.rate).round() as u64);
            }
        }
    }
    onsets.sort_unstable();
    let mut restarts: Vec<u64> = Vec::new();
    for onset in onsets {
        let Some(at) = onset.checked_sub(ANCHOR_LEAD).filter(|at| *at > 0) else { continue };
        if restarts.last().is_none_or(|last| at >= last + min_gap) {
            restarts.push(at);
        }
    }
    restarts
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

    /// Renders a warped click clip and returns how far each click lands from where the warp
    /// markers put it, in milliseconds.
    fn click_errors(markers: &[(f64, f64)], length_beats: f64) -> Vec<f64> {
        let sr = 48_000;
        let data = clicks(0.5, 40.0, sr);
        let r = region(markers, StretchMode::Smooth);
        let c = clip(length_beats, 0.0, None);
        let analysis = analyze_audio(&data);
        let mut voice = StretchVoice::new(&c, &r, data, Some(&analysis), 120.0, sr).unwrap();
        let frames = (length_beats * 0.5 * f64::from(sr)) as usize;
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
            .map(|&t| {
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

    #[test]
    fn plays_through_at_any_device_rate_and_block_size() {
        // A 48 kHz file on 44.1 kHz and 96 kHz devices, with restarts, in odd block sizes.
        let data = clicks(0.25, 40.0, 48_000);
        let analysis = analyze_audio(&data);
        for markers in [vec![(0.0, 0.0), (64.0, 30.4)], vec![(0.0, 0.0), (64.0, 45.0)]] {
            let r = region(&markers, StretchMode::Smooth);
            for (device_rate, block) in [(44_100, 441), (44_100, 448), (96_000, 37), (22_050, 1000)] {
                let mut voice =
                    StretchVoice::new(&clip(60.0, 0.0, None), &r, data.clone(), Some(&analysis), 120.0, device_rate)
                        .unwrap();
                let mut out = vec![0.0; block * 2];
                for _ in 0..(30 * device_rate as usize / block) {
                    voice.render(&mut out);
                }
            }
        }
    }

    #[test]
    fn keylock_keeps_transients_on_the_grid() {
        for markers in [
            vec![(0.0, 0.0), (64.0, 30.4)],
            vec![(0.0, 0.0), (64.0, 33.6)],
            vec![(0.0, 0.0), (8.0, 3.9), (16.0, 8.1), (24.0, 11.9), (32.0, 16.2), (48.0, 23.8)],
        ] {
            let errors = click_errors(&markers, 46.0);
            assert!(errors.len() >= 40, "{} clicks found", errors.len());
            // Without the restarts transients drift up to ~9 ms, enough to flam against other tracks.
            assert!(worst(&errors) < 3.0, "{markers:?}: {errors:?}");
        }
    }

    #[test]
    fn phase_vocoder_keeps_transients_near_the_grid() {
        for rate in [0.6, 0.75, 1.3, 1.5] {
            let errors = click_errors(&[(0.0, 0.0), (64.0, 32.0 * rate)], 30.0);
            assert!(errors.len() >= 15, "{} clicks found", errors.len());
            let mean = errors.iter().sum::<f64>() / errors.len() as f64;
            // Its window smears clicks by a few ms either way; without the corrections the clicks
            // land 10-30 ms off, depending on the rate.
            assert!(mean.abs() < 7.0, "rate {rate}: mean {mean:.1} ms, {errors:?}");
        }
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
