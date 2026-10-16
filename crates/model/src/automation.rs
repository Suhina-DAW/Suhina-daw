// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation envelopes: the value of an [`AutomationLane`] at a beat, and the mixer parameters a
//! channel's lanes can name. Pure functions, shared by the engine, the commands and the tests
//! (`docs/spec/automation.md`).

use crate::{AutomationLane, AutomationPoint, Channel};

/// The mixer parameters of a channel that a lane can automate: its volume (linear gain, 0 up to
/// [`MAX_VOLUME`]), its pan (-1 to 1) and its mute (0 or 1, a step).
pub const MIXER_PARAMS: [&str; 3] = [VOLUME, PAN, MUTE];
pub const VOLUME: &str = "volume";
pub const PAN: &str = "pan";
pub const MUTE: &str = "mute";
/// The loudest a volume lane goes: +6 dB, as a fader.
pub const MAX_VOLUME: f64 = 1.995_262_314_968_879_6;
/// Most points one lane holds.
pub const MAX_POINTS: usize = 65_536;

/// How far a segment of shape `curve` (-1 to 1) has moved at `t` (0 to 1) of its length: `t`
/// itself for 0, `t` to the power of 4^-curve otherwise, so 1 moves fast first and settles,
/// -1 starts slowly and rises late. Out-of-range and non-finite input is held in range.
pub fn shape(t: f64, curve: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let curve = if curve.is_finite() { curve.clamp(-1.0, 1.0) } else { 0.0 };
    if curve == 0.0 { t } else { t.powf(4f64.powf(-curve)) }
}

/// The value of `points` (in beat order) at `beat`: the first point's before it, the last's
/// after it, and along each segment's curve between; at a beat with two points (a jump) the
/// later one's. With `stepped`, a segment holds its first point's value up to the next point
/// (for on/off and choice parameters). `None` for no points.
pub fn value_at(points: &[AutomationPoint], beat: f64, stepped: bool) -> Option<f64> {
    let first = points.first()?;
    // Before the first point, and for a beat that is not a number.
    if beat.partial_cmp(&first.beat).is_none_or(|o| o == std::cmp::Ordering::Less) {
        return Some(first.value);
    }
    // The last point at or before `beat`.
    let index = points.partition_point(|p| p.beat <= beat).saturating_sub(1);
    let a = points.get(index)?;
    let Some(b) = points.get(index + 1) else { return Some(a.value) };
    let span = b.beat - a.beat;
    if stepped || span.is_nan() || span <= 0.0 {
        return Some(a.value);
    }
    let t = (beat - a.beat) / span;
    Some(a.value + (b.value - a.value) * shape(t, a.curve))
}

impl AutomationLane {
    /// An enabled lane of `param` with no points.
    pub fn new(param: impl Into<String>) -> Self {
        Self { param: param.into(), points: Vec::new(), enabled: true }
    }

    /// Puts a point in beat order, after any at the same beat, and returns its index.
    pub fn insert(&mut self, point: AutomationPoint) -> usize {
        let index = self.points.partition_point(|p| p.beat <= point.beat);
        self.points.insert(index, point);
        index
    }

    /// The lane's value at `beat` (see [`value_at`]).
    pub fn value_at(&self, beat: f64, stepped: bool) -> Option<f64> {
        value_at(&self.points, beat, stepped)
    }
}

impl AutomationPoint {
    /// A point on a straight segment.
    pub fn new(beat: f64, value: f64) -> Self {
        Self { beat, value, curve: 0.0 }
    }
}

impl Channel {
    /// The channel's lane for mixer parameter `param`, if it has one.
    pub fn lane(&self, param: &str) -> Option<&AutomationLane> {
        self.automation.iter().find(|l| l.param == param)
    }

    /// Whether `param` follows an enabled lane with points.
    pub fn automated(&self, param: &str) -> bool {
        self.lane(param).is_some_and(|l| l.enabled && !l.points.is_empty())
    }
}

/// The range a mixer parameter's values are kept in, `None` for a name that is not one.
pub fn mixer_range(param: &str) -> Option<(f64, f64)> {
    match param {
        VOLUME => Some((0.0, MAX_VOLUME)),
        PAN => Some((-1.0, 1.0)),
        MUTE => Some((0.0, 1.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(list: &[(f64, f64)]) -> Vec<AutomationPoint> {
        list.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect()
    }

    #[test]
    fn a_lane_holds_outside_its_points_and_moves_in_a_line_between() {
        let p = points(&[(1.0, 0.0), (3.0, 1.0)]);
        assert_eq!(value_at(&p, 0.0, false), Some(0.0));
        assert_eq!(value_at(&p, 2.0, false), Some(0.5));
        assert_eq!(value_at(&p, 10.0, false), Some(1.0));
        assert_eq!(value_at(&p, 2.0, true), Some(0.0));
        assert_eq!(value_at(&[], 2.0, false), None);
    }

    #[test]
    fn two_points_at_one_beat_jump_there() {
        let p = points(&[(0.0, 0.0), (2.0, 0.2), (2.0, 0.8), (4.0, 0.8)]);
        assert!((value_at(&p, 1.999, false).unwrap_or(0.0) - 0.2).abs() < 1e-3);
        assert_eq!(value_at(&p, 2.0, false), Some(0.8));
    }

    #[test]
    fn curves_bend_a_segment_both_ways() {
        assert_eq!(shape(0.25, 0.0), 0.25);
        assert!(shape(0.25, 1.0) > 0.5);
        assert!(shape(0.25, -1.0) < 0.01);
        assert_eq!(shape(1.0, 0.7), 1.0);
        assert_eq!(shape(f64::NAN, 1.0), 0.0);
        assert_eq!(shape(0.5, f64::INFINITY), 0.5, "a curve that is not a number is straight");
        let mut p = points(&[(0.0, 0.0), (1.0, 1.0)]);
        if let Some(first) = p.first_mut() {
            first.curve = 0.5;
        }
        assert!(value_at(&p, 0.5, false).unwrap_or(0.0) > 0.5);
    }

    #[test]
    fn hostile_beats_never_panic() {
        let p = points(&[(0.0, 1.0), (f64::MAX, 2.0)]);
        for beat in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, f64::MAX] {
            assert!(value_at(&p, beat, false).is_some());
        }
    }

    #[test]
    fn points_go_in_beat_order_after_those_at_the_same_beat() {
        let mut lane = AutomationLane::new(VOLUME);
        assert_eq!(lane.insert(AutomationPoint::new(2.0, 1.0)), 0);
        assert_eq!(lane.insert(AutomationPoint::new(1.0, 0.0)), 0);
        assert_eq!(lane.insert(AutomationPoint::new(2.0, 0.5)), 2);
        assert_eq!(lane.points.iter().map(|p| p.value).collect::<Vec<_>>(), [0.0, 1.0, 0.5]);
    }
}
