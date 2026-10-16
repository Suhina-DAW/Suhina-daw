// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tempo map and the meter: how beats turn into time and into bars when the tempo and the
//! time signature change over the song (`docs/spec/tempo-map.md`).
//!
//! [`TempoMap`] is the one conversion between beats and time. It counts time in any unit: seconds
//! (`units_per_second` 1) for the UI, frames (the sample rate) for the engine. Ramps are
//! converted with their exact closed form, so a position is computed from the map, never added
//! up frame by frame, and a long ramp does not drift. For a constant tempo the arithmetic is the
//! one the engine always used (`beat * 60 / bpm * rate`), so renders do not change.
//!
//! [`Meter`] turns beats into bars through the time-signature changes. Both are built off the
//! audio thread (they allocate) and only read there (they do not).

use crate::{TimeSignature, Transport};

/// The slowest and fastest tempo a project plays, in beats per minute.
pub const MIN_BPM: f64 = 1.0;
pub const MAX_BPM: f64 = 999.0;
/// Most tempo changes and time-signature changes a project keeps; later ones are ignored, so a
/// malformed file cannot make every conversion search an unbounded list.
pub const MAX_TEMPO_CHANGES: usize = 10_000;
pub const MAX_TIME_SIGNATURE_CHANGES: usize = 10_000;
/// The latest beat a tempo change can be at: over 16 hours at 120 bpm.
pub const MAX_CHANGE_BEAT: f64 = 120_000.0;

/// A tempo change at `beat`. With `ramp` the tempo glides from the tempo before (the previous
/// change's, or the start tempo) to `bpm`, reaching it at `beat`, linearly in beats; without it
/// the tempo steps to `bpm` at `beat`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
    pub ramp: bool,
}

/// A time-signature change at the start of bar `bar` (counted from 0: the song's first bar is
/// bar 0, shown as bar 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignatureChange {
    pub bar: u32,
    pub time_signature: TimeSignature,
}

/// Whether `bpm` is a tempo a project can play.
pub fn valid_bpm(bpm: f64) -> bool {
    bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(&bpm)
}

impl TimeSignature {
    /// Beats (quarter notes) in one bar: a 4/4 bar has 4, a 6/8 bar 3. A signature with a zero
    /// part counts as 4/4.
    pub fn bar_beats(self) -> f64 {
        if self.numerator == 0 || self.denominator == 0 {
            return 4.0;
        }
        f64::from(self.numerator) * 4.0 / f64::from(self.denominator)
    }

    /// Whether this is a signature a project keeps: a numerator from 1 to 99 and a denominator of
    /// 1, 2, 4, 8, 16, 32 or 64.
    pub fn is_valid(self) -> bool {
        (1..=99).contains(&self.numerator) && self.denominator.is_power_of_two() && self.denominator <= 64
    }
}

/// One stretch of the map, from `beat` to the next segment's beat: at a constant tempo, or
/// ramping linearly in beats from `bpm` to `end_bpm` over `length` beats.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    beat: f64,
    /// Time at `beat`, in the map's units.
    at: f64,
    bpm: f64,
    /// Units per beat at `bpm`: `60 / bpm * units_per_second`.
    per_beat: f64,
    /// For a ramp: the change in bpm per beat. Zero for a constant tempo.
    slope: f64,
}

/// Beats to time and back through a project's tempo changes.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    segments: Vec<Segment>,
    units_per_second: f64,
}

impl TempoMap {
    /// A map at one tempo throughout. `bpm` must be valid ([`valid_bpm`]); otherwise the map
    /// plays at 120.
    pub fn constant(bpm: f64, units_per_second: f64) -> Self {
        Self::new(bpm, &[], units_per_second)
    }

    /// The map of a start tempo and its changes. Changes that are not finite, not after beat 0,
    /// past [`MAX_CHANGE_BEAT`] or at a tempo outside 1 to 999 bpm are ignored, as is everything
    /// past the first [`MAX_TEMPO_CHANGES`]; of two changes on the same beat the later one in
    /// the list wins.
    pub fn new(start_bpm: f64, changes: &[TempoPoint], units_per_second: f64) -> Self {
        let units_per_second =
            if units_per_second.is_finite() && units_per_second > 0.0 { units_per_second } else { 1.0 };
        let start_bpm = if valid_bpm(start_bpm) { start_bpm } else { 120.0 };
        let mut points: Vec<TempoPoint> = changes
            .iter()
            .take(MAX_TEMPO_CHANGES)
            .filter(|p| p.beat.is_finite() && p.beat > 0.0 && p.beat <= MAX_CHANGE_BEAT && valid_bpm(p.bpm))
            .copied()
            .collect();
        // Stable, so points on one beat keep their order. Of two on one beat the later wins,
        // except a ramp followed by a step: the ramp arrives, then the tempo jumps.
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let mut kept: Vec<TempoPoint> = Vec::with_capacity(points.len());
        for point in points {
            let len = kept.len();
            let same = |p: Option<&TempoPoint>| p.is_some_and(|p| p.beat == point.beat);
            if !same(kept.last()) {
                kept.push(point);
                continue;
            }
            let ramp_then_step = kept.last().is_some_and(|l| l.ramp) && !point.ramp;
            if ramp_then_step && !same(len.checked_sub(2).and_then(|i| kept.get(i))) {
                kept.push(point);
            } else if let Some(last) = kept.last_mut() {
                *last = point;
            }
        }
        let per_beat = |bpm: f64| 60.0 / bpm * units_per_second;
        let mut segments = Vec::with_capacity(kept.len() + 1);
        let mut current = Segment { beat: 0.0, at: 0.0, bpm: start_bpm, per_beat: per_beat(start_bpm), slope: 0.0 };
        for point in &kept {
            // A ramp into `point` makes the segment before it a ramp.
            if point.ramp && point.bpm != current.bpm && point.beat > current.beat {
                current.slope = (point.bpm - current.bpm) / (point.beat - current.beat);
            }
            let at = current.at + current.units_after(point.beat - current.beat, units_per_second);
            segments.push(current);
            // Constant until a ramp into the next point says otherwise; the last stays constant.
            current = Segment { beat: point.beat, at, bpm: point.bpm, per_beat: per_beat(point.bpm), slope: 0.0 };
        }
        segments.push(current);
        Self { segments, units_per_second }
    }

    /// Whether the tempo never changes.
    pub fn is_constant(&self) -> bool {
        self.segments.len() == 1
    }

    /// The tempo at the start, in bpm.
    pub fn start_bpm(&self) -> f64 {
        self.segments.first().map_or(120.0, |s| s.bpm)
    }

    /// Units per beat at the start tempo.
    pub fn start_per_beat(&self) -> f64 {
        self.segments.first().map_or(0.0, |s| s.per_beat)
    }

    /// The segment that holds `beat` (the first for a beat before 0).
    fn segment_at_beat(&self, beat: f64) -> Option<&Segment> {
        let index = self.segments.partition_point(|s| s.beat <= beat).max(1) - 1;
        self.segments.get(index)
    }

    /// The segment that holds time `units` (the first for a time before 0).
    fn segment_at_time(&self, units: f64) -> Option<&Segment> {
        let index = self.segments.partition_point(|s| s.at <= units).max(1) - 1;
        self.segments.get(index)
    }

    /// The time of `beat`, in the map's units. Beats before 0 extend the start tempo backwards.
    /// Never allocates.
    pub fn time_at(&self, beat: f64) -> f64 {
        let Some(segment) = self.segment_at_beat(beat) else { return 0.0 };
        segment.at + segment.units_after(beat - segment.beat, self.units_per_second)
    }

    /// The beat at time `units`; the inverse of [`TempoMap::time_at`]. Never allocates.
    pub fn beat_at(&self, units: f64) -> f64 {
        let Some(segment) = self.segment_at_time(units) else { return 0.0 };
        segment.beat + segment.beats_after(units - segment.at, self.units_per_second)
    }

    /// The tempo at `beat`, in bpm.
    pub fn bpm_at(&self, beat: f64) -> f64 {
        let Some(segment) = self.segment_at_beat(beat) else { return 120.0 };
        if segment.slope == 0.0 { segment.bpm } else { segment.bpm + segment.slope * (beat - segment.beat).max(0.0) }
    }

    /// The time from `beat` to `beat + beats`, in the map's units. At a constant tempo this is
    /// `beats * per_beat` exactly, as the engine has always worked it out.
    pub fn span(&self, beat: f64, beats: f64) -> f64 {
        if self.is_constant() {
            return beats * self.start_per_beat();
        }
        self.time_at(beat + beats) - self.time_at(beat)
    }

    /// The beats at which the tempo changes (steps, and where ramps begin and end), in order.
    pub fn breaks(&self) -> impl Iterator<Item = f64> + '_ {
        self.segments.iter().skip(1).map(|s| s.beat)
    }

    /// The first beat after `beat` at which the tempo changes (see [`TempoMap::breaks`]), if
    /// any. A binary search; never allocates.
    pub fn next_break_after(&self, beat: f64) -> Option<f64> {
        let index = self.segments.partition_point(|s| s.beat <= beat);
        self.segments.get(index).map(|s| s.beat)
    }

    /// Whether the tempo is ramping at `beat`.
    pub fn ramps_at(&self, beat: f64) -> bool {
        self.segment_at_beat(beat).is_some_and(|s| s.slope != 0.0)
    }
}

impl Segment {
    /// Units from this segment's start to `beats` beats after it.
    fn units_after(&self, beats: f64, units_per_second: f64) -> f64 {
        if self.slope == 0.0 || beats <= 0.0 {
            return beats * self.per_beat;
        }
        // dt/db = 60 / (bpm + slope * b), so t = 60 / slope * ln(1 + slope * b / bpm).
        let seconds = 60.0 / self.slope * (self.slope * beats / self.bpm).ln_1p();
        if seconds.is_finite() { seconds * units_per_second } else { beats * self.per_beat }
    }

    /// Beats from this segment's start to `units` after it; the inverse of `units_after`.
    fn beats_after(&self, units: f64, units_per_second: f64) -> f64 {
        if self.slope == 0.0 || units <= 0.0 {
            return if self.per_beat > 0.0 { units / self.per_beat } else { 0.0 };
        }
        let seconds = units / units_per_second;
        let beats = self.bpm / self.slope * (self.slope * seconds / 60.0).exp_m1();
        if beats.is_finite() { beats } else { units / self.per_beat }
    }
}

/// One stretch of bars at one time signature, from bar `bar` (which starts at `beat`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct MeterSegment {
    bar: u32,
    beat: f64,
    bar_beats: f64,
    time_signature: TimeSignature,
}

/// Where a beat is in the bars: the bar (from 0), the beat its bar starts at, the bar's length in
/// beats and its time signature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarPosition {
    pub bar: u32,
    pub bar_start: f64,
    pub bar_beats: f64,
    pub time_signature: TimeSignature,
}

/// Beats to bars and back through a project's time-signature changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    segments: Vec<MeterSegment>,
}

impl Default for Meter {
    /// 4/4 throughout.
    fn default() -> Self {
        Self::new(TimeSignature { numerator: 4, denominator: 4 }, &[])
    }
}

impl Meter {
    /// The meter of a start signature and its changes. Changes at bar 0, with an invalid
    /// signature ([`TimeSignature::is_valid`]) or past [`MAX_TIME_SIGNATURE_CHANGES`] are
    /// ignored; of two on the same bar the later one in the list wins.
    pub fn new(start: TimeSignature, changes: &[TimeSignatureChange]) -> Self {
        let mut kept: Vec<TimeSignatureChange> = changes
            .iter()
            .take(MAX_TIME_SIGNATURE_CHANGES)
            .filter(|c| c.bar > 0 && c.time_signature.is_valid())
            .copied()
            .collect();
        kept.sort_by_key(|c| c.bar);
        let mut deduped: Vec<TimeSignatureChange> = Vec::with_capacity(kept.len());
        for change in kept {
            match deduped.last_mut() {
                Some(last) if last.bar == change.bar => *last = change,
                _ => deduped.push(change),
            }
        }
        let mut segments = Vec::with_capacity(deduped.len() + 1);
        let mut current = MeterSegment { bar: 0, beat: 0.0, bar_beats: start.bar_beats(), time_signature: start };
        for change in deduped {
            let beat = current.beat + f64::from(change.bar - current.bar) * current.bar_beats;
            segments.push(current);
            current = MeterSegment {
                bar: change.bar,
                beat,
                bar_beats: change.time_signature.bar_beats(),
                time_signature: change.time_signature,
            };
        }
        segments.push(current);
        Self { segments }
    }

    /// Whether the time signature never changes.
    pub fn is_constant(&self) -> bool {
        self.segments.len() == 1
    }

    /// Beats in a bar at the start.
    pub fn start_bar_beats(&self) -> f64 {
        self.segments.first().map_or(4.0, |s| s.bar_beats)
    }

    /// The bar that holds `beat` (bar 0 for a beat before 0). Never allocates.
    pub fn position(&self, beat: f64) -> BarPosition {
        let index = self.segments.partition_point(|s| s.beat <= beat).max(1) - 1;
        let Some(s) = self.segments.get(index) else {
            return BarPosition {
                bar: 0,
                bar_start: 0.0,
                bar_beats: 4.0,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            };
        };
        let bars = if s.bar_beats > 0.0 && beat > s.beat { ((beat - s.beat) / s.bar_beats).floor() } else { 0.0 };
        // Bars past u32 are far beyond any project: stay on the last one.
        let bars = bars.min(f64::from(u32::MAX - s.bar));
        BarPosition {
            bar: s.bar + bars as u32,
            bar_start: s.beat + bars * s.bar_beats,
            bar_beats: s.bar_beats,
            time_signature: s.time_signature,
        }
    }

    /// The beat bar `bar` starts at.
    pub fn bar_start(&self, bar: u32) -> f64 {
        let index = self.segments.partition_point(|s| s.bar <= bar).max(1) - 1;
        self.segments.get(index).map_or(0.0, |s| s.beat + f64::from(bar - s.bar) * s.bar_beats)
    }

    /// The time-signature changes the meter kept, in order (without the start).
    pub fn changes(&self) -> impl Iterator<Item = (u32, f64, TimeSignature)> + '_ {
        self.segments.iter().skip(1).map(|s| (s.bar, s.beat, s.time_signature))
    }
}

impl Transport {
    /// The tempo map of this transport, counting time in `units_per_second` (1 for seconds, the
    /// sample rate for frames).
    pub fn tempo_map(&self, units_per_second: f64) -> TempoMap {
        TempoMap::new(self.tempo_bpm, &self.tempo_changes, units_per_second)
    }

    /// The meter of this transport.
    pub fn meter(&self) -> Meter {
        Meter::new(self.time_signature, &self.time_signature_changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(numerator: u8, denominator: u8) -> TimeSignature {
        TimeSignature { numerator, denominator }
    }

    fn point(beat: f64, bpm: f64, ramp: bool) -> TempoPoint {
        TempoPoint { beat, bpm, ramp }
    }

    #[test]
    fn a_constant_map_converts_exactly_as_before() {
        let rate = 48_000.0;
        let map = TempoMap::constant(123.0, rate);
        let per_beat = 60.0 / 123.0 * rate;
        for beat in [0.0, 0.25, 1.0, 7.3, 1234.5678] {
            // Bit for bit: golden renders depend on it.
            assert_eq!(map.time_at(beat), beat * per_beat);
            assert_eq!(map.span(3.0, beat), beat * per_beat);
            assert_eq!(map.beat_at(beat * 1000.0), beat * 1000.0 / per_beat);
        }
        assert!(map.is_constant());
        assert_eq!(map.bpm_at(50.0), 123.0);
    }

    #[test]
    fn a_step_changes_the_tempo_at_its_beat() {
        // 120 bpm for 4 beats (2 s), then 60 bpm.
        let map = TempoMap::new(120.0, &[point(4.0, 60.0, false)], 1.0);
        assert_eq!(map.time_at(4.0), 2.0);
        assert_eq!(map.time_at(6.0), 4.0);
        assert_eq!(map.beat_at(4.0), 6.0);
        assert_eq!(map.bpm_at(3.999), 120.0);
        assert_eq!(map.bpm_at(4.0), 60.0);
        assert_eq!(map.span(3.0, 2.0), 1.5);
    }

    #[test]
    fn a_ramp_matches_its_closed_form_and_round_trips() {
        // From 120 at beat 0 to 180 at beat 8: dt/db = 60 / (120 + 7.5 b).
        let map = TempoMap::new(120.0, &[point(8.0, 180.0, true)], 1.0);
        let expected = 60.0 / 7.5 * (180.0f64 / 120.0).ln();
        assert!((map.time_at(8.0) - expected).abs() < 1e-12);
        assert!((map.bpm_at(4.0) - 150.0).abs() < 1e-12);
        assert!(map.ramps_at(0.0) && map.ramps_at(7.9) && !map.ramps_at(8.0));
        assert_eq!(map.bpm_at(9.0), 180.0);
        // After the ramp, a constant 180 bpm.
        assert!((map.time_at(10.0) - (expected + 2.0 / 3.0)).abs() < 1e-12);
        for beat in [0.0, 0.5, 3.0, 7.99, 8.0, 12.0, 1000.0] {
            let back = map.beat_at(map.time_at(beat));
            assert!((back - beat).abs() < 1e-9, "{beat} came back as {back}");
        }
        // A downward ramp too.
        let map = TempoMap::new(140.0, &[point(4.0, 70.0, true)], 48_000.0);
        for beat in [0.1, 2.0, 3.9, 4.0, 5.0] {
            assert!((map.beat_at(map.time_at(beat)) - beat).abs() < 1e-9);
        }
    }

    #[test]
    fn several_changes_add_up() {
        // 120 for 4 beats (2 s), 60 for 2 (2 s), then a ramp from 60 to 120 over 4 beats and a
        // step down to 30.
        let changes =
            [point(4.0, 60.0, false), point(10.0, 120.0, true), point(6.0, 60.0, false), point(12.0, 30.0, false)];
        let map = TempoMap::new(120.0, &changes, 1.0);
        let ramp = 60.0 / 15.0 * 2f64.ln();
        assert_eq!(map.time_at(6.0), 4.0);
        assert!((map.time_at(10.0) - (4.0 + ramp)).abs() < 1e-12);
        assert!((map.time_at(11.0) - (4.0 + ramp + 0.5)).abs() < 1e-12);
        assert!((map.time_at(13.0) - (4.0 + ramp + 1.0 + 2.0)).abs() < 1e-12);
        let mut last = -1.0;
        for i in 0..200 {
            let beat = f64::from(i) * 0.1;
            let time = map.time_at(beat);
            assert!(time > last, "time goes backwards at beat {beat}");
            assert!((map.beat_at(time) - beat).abs() < 1e-9);
            last = time;
        }
    }

    #[test]
    fn a_ramp_can_end_in_a_jump() {
        // A glide from 120 to 180 over 8 beats that then jumps to 60.
        let map = TempoMap::new(120.0, &[point(8.0, 180.0, true), point(8.0, 60.0, false)], 1.0);
        let glide = 60.0 / 7.5 * 1.5f64.ln();
        assert!((map.time_at(8.0) - glide).abs() < 1e-12);
        assert!((map.time_at(9.0) - (glide + 1.0)).abs() < 1e-12);
        assert!((map.bpm_at(7.999) - 180.0).abs() < 0.01);
        assert_eq!(map.bpm_at(8.0), 60.0);
        assert!((map.beat_at(glide + 0.5) - 8.5).abs() < 1e-9);
        // A step then a ramp on one beat: the later wins, as for any other pair.
        let map = TempoMap::new(120.0, &[point(8.0, 60.0, false), point(8.0, 180.0, true)], 1.0);
        assert_eq!(map.breaks().count(), 1);
    }

    #[test]
    fn a_long_ramp_does_not_drift() {
        // Summing a ramp frame by frame drifts; the closed form agrees with a fine numerical
        // integral to well under a frame after many minutes.
        let rate = 48_000.0;
        let map = TempoMap::new(60.0, &[point(1000.0, 200.0, true)], rate);
        let steps = 1_000_000;
        let mut frames = 0.0;
        for i in 0..steps {
            let b = (f64::from(i) + 0.5) * 1000.0 / f64::from(steps);
            frames += 1000.0 / f64::from(steps) * 60.0 / map.bpm_at(b) * rate;
        }
        assert!((map.time_at(1000.0) - frames).abs() < 0.01, "{} vs {frames}", map.time_at(1000.0));
    }

    #[test]
    fn malformed_changes_are_ignored() {
        let changes = [
            point(f64::NAN, 100.0, false),
            point(-1.0, 100.0, false),
            point(0.0, 100.0, false),
            point(4.0, 0.0, false),
            point(4.0, f64::INFINITY, true),
            point(1e300, 100.0, false),
            point(8.0, 90.0, false),
            point(8.0, 60.0, false),
        ];
        let map = TempoMap::new(f64::NAN, &changes, f64::NAN);
        assert_eq!(map.start_bpm(), 120.0);
        assert_eq!(map.breaks().collect::<Vec<_>>(), vec![8.0]);
        assert_eq!((map.next_break_after(2.0), map.next_break_after(8.0)), (Some(8.0), None));
        assert_eq!(map.bpm_at(9.0), 60.0, "the later of two points on one beat wins");
        let many: Vec<TempoPoint> = (1..=20_000).map(|i| point(f64::from(i), 100.0, false)).collect();
        assert_eq!(TempoMap::new(120.0, &many, 1.0).breaks().count(), MAX_TEMPO_CHANGES);
    }

    #[test]
    fn the_meter_counts_bars_through_changes() {
        // 4/4 for bars 0 and 1 (8 beats), 3/4 from bar 2, 6/8 from bar 4.
        let meter = Meter::new(
            ts(4, 4),
            &[
                TimeSignatureChange { bar: 4, time_signature: ts(6, 8) },
                TimeSignatureChange { bar: 2, time_signature: ts(3, 4) },
            ],
        );
        assert_eq!(meter.bar_start(2), 8.0);
        assert_eq!(meter.bar_start(3), 11.0);
        assert_eq!(meter.bar_start(4), 14.0);
        assert_eq!(meter.bar_start(6), 20.0);
        let p = meter.position(12.0);
        assert_eq!((p.bar, p.bar_start, p.bar_beats), (3, 11.0, 3.0));
        let p = meter.position(15.5);
        assert_eq!((p.bar, p.bar_start, p.time_signature), (4, 14.0, ts(6, 8)));
        assert_eq!(meter.position(-3.0).bar, 0);
        assert_eq!(meter.changes().map(|c| c.0).collect::<Vec<_>>(), vec![2, 4]);
    }

    #[test]
    fn malformed_signature_changes_are_ignored() {
        let meter = Meter::new(
            ts(4, 4),
            &[
                TimeSignatureChange { bar: 0, time_signature: ts(3, 4) },
                TimeSignatureChange { bar: 2, time_signature: ts(0, 4) },
                TimeSignatureChange { bar: 3, time_signature: ts(3, 3) },
                TimeSignatureChange { bar: u32::MAX, time_signature: ts(5, 4) },
            ],
        );
        assert_eq!(meter.changes().count(), 1);
        assert_eq!(meter.position(1e300).bar, u32::MAX);
        assert!(ts(7, 8).is_valid() && !ts(100, 4).is_valid() && !ts(4, 128).is_valid());
        assert_eq!(ts(0, 4).bar_beats(), 4.0);
    }
}
