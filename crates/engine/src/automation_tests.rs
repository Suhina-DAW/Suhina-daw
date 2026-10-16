// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_model::AutomationPoint;

fn lane(param: &str, points: &[(f64, f64)]) -> AutomationLane {
    AutomationLane {
        param: param.into(),
        points: points.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect(),
        enabled: true,
    }
}

#[test]
fn an_envelope_is_built_from_an_enabled_lane_with_finite_points() {
    let mut l = lane("gain", &[(1.0, 2.0), (f64::NAN, 1.0), (0.5, f64::INFINITY), (2.0, 4.0)]);
    let envelope = Envelope::new(&l, 100.0, false).unwrap();
    assert_eq!(envelope.value_at(0), 2.0);
    assert_eq!(envelope.value_at(150), 3.0);
    assert_eq!(envelope.value_at(u64::MAX), 4.0);
    let stepped = Envelope::new(&l, 100.0, true).unwrap();
    assert_eq!(stepped.value_at(199), 2.0);
    l.enabled = false;
    assert!(Envelope::new(&l, 100.0, false).is_none());
    assert!(Envelope::new(&lane("gain", &[(f64::NAN, 1.0)]), 100.0, false).is_none());
}

#[test]
fn device_changes_fall_on_the_arrangements_grid_and_at_points() {
    let device = suhina_device_registry::create("utility").unwrap();
    let mut lanes = DeviceLanes::none();
    // 1 beat = 100 frames; a ramp from frame 100 to 200 and a jump at 300.
    lanes.add(0, device.as_ref(), &[lane("gain", &[(1.0, 0.0), (2.0, -10.0), (3.0, -10.0), (3.0, -20.0)])], 100.0);
    lanes.add(0, device.as_ref(), &[lane("no-such-param", &[(0.0, 1.0)])], 100.0);
    assert!(lanes.covers(0, 0) && !lanes.covers(1, 0));
    lanes.plan(None, 512);
    assert_eq!(lanes.changes(0, 0, 512).count(), 0, "stopped: nothing");
    // From frame 40: the first frame, then the grid of the arrangement (64, 128, ...) where
    // the value moves, and the points.
    lanes.plan(Some(40), 512);
    let frames: Vec<usize> = lanes.changes(0, 0, 512).map(|c| c.frame).collect();
    assert_eq!(frames, [0, 88, 152, 160, 260]);
    let last = lanes.changes(0, 0, 512).last().map(|c| c.value);
    assert_eq!(last, Some(-20.0));
    // The same block again: only what changed (nothing, the value holds after the jump).
    lanes.plan(Some(552), 512);
    assert_eq!(lanes.changes(0, 0, 512).count(), 0);
    assert_eq!(lanes.changes(0, 0, 100).count(), 0);
}

#[test]
fn hostile_device_lanes_stay_within_bounds() {
    let device = suhina_device_registry::create("utility").unwrap();
    let mut lanes = DeviceLanes::none();
    let many: Vec<(f64, f64)> = (0..5_000).map(|i| (f64::from(i) * 0.001, f64::from(i % 2) * -10.0)).collect();
    for slot in 0..100 {
        lanes.add(slot, device.as_ref(), &[lane("gain", &many), lane("width", &[(1e300, 0.5), (-5.0, 1.0)])], 22_050.0);
    }
    for time in [0, 1, u64::MAX / 2, u64::MAX - 10] {
        lanes.plan(Some(time), 1024);
        assert!(lanes.block.len() <= MAX_EVENTS);
        assert!(lanes.block.windows(2).all(|w| w[0].frame <= w[1].frame));
        assert!(lanes.block.iter().all(|c| c.frame < 1024 && c.value.is_finite()));
    }
}

fn channel(lanes: Vec<AutomationLane>) -> Channel {
    Channel {
        id: suhina_model::ChannelId(1),
        name: "C".into(),
        role: ChannelRole::Regular,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        automation: lanes,
        extensions: Default::default(),
    }
}

#[test]
fn mixer_gains_are_a_line_between_grid_values_in_any_block() {
    let c = channel(vec![lane("volume", &[(0.0, 0.0), (10.0, 1.0)]), lane("pan", &[(0.0, 0.0)])]);
    let mut mix = MixLanes::new(&c, 100.0, true).unwrap();
    // Whole: every frame's gain is its time / 1000, centred, up to the step the lane's last
    // point falls in (a corner inside a step is cut across it).
    mix.plan(0, 1024, true);
    let whole: Vec<(f32, f32)> = (0..1024).map(|f| mix.gains(f)).collect();
    assert!(
        whole
            .iter()
            .take(960)
            .enumerate()
            .all(|(f, g)| (g.0 - f as f32 / 1000.0).abs() < 1e-5 && (g.0 - g.1).abs() < 1e-5)
    );
    assert!((whole.get(992).map_or(0.0, |g| g.0) - 0.98).abs() < 1e-5);
    // In pieces of 333 frames: the same.
    for start in (0..1024).step_by(333) {
        mix.plan(start as u64, 333, true);
        for f in 0..333.min(1024 - start) {
            assert_eq!(mix.gains(f), whole[start + f], "frame {}", start + f);
        }
    }
    // Stopped: the value at the play start marker throughout.
    mix.plan(500, 64, false);
    assert!((0..64).all(|f| (mix.gains(f).0 - 0.5).abs() < 1e-6));
    // Latency compensation reads the lanes earlier.
    mix.set_latency(100);
    mix.plan(500, 1, true);
    assert!((mix.gains(0).0 - 0.4).abs() < 1e-6);
}

#[test]
fn a_mute_lane_silences_and_pan_follows_only_where_it_applies() {
    let c = channel(vec![lane("mute", &[(0.0, 1.0), (1.0, 0.0)]), lane("pan", &[(0.0, 1.0)])]);
    let mut mix = MixLanes::new(&c, 64.0, true).unwrap();
    mix.plan(0, 256, true);
    assert_eq!(mix.gains(0), (0.0, 0.0));
    let (l, r) = mix.gains(200);
    assert!(l.abs() < 1e-6 && (r - std::f32::consts::SQRT_2).abs() < 1e-5);
    // On a group's bus the pan does not apply.
    let mut bus = MixLanes::new(&c, 64.0, false).unwrap();
    bus.plan(0, 256, true);
    assert_eq!(bus.gains(200), (1.0, 1.0));
    // No lane that plays: none.
    assert!(MixLanes::new(&channel(vec![]), 64.0, true).is_none());
    let mut off = lane("volume", &[(0.0, 0.5)]);
    off.enabled = false;
    assert!(MixLanes::new(&channel(vec![off]), 64.0, true).is_none());
}

#[test]
fn a_neutral_project_has_the_automated_mixer_parameters_at_rest() {
    let mut project = Project::default();
    assert!(neutral(&project).is_none());
    let mut c = channel(vec![lane("volume", &[(0.0, 0.5)]), lane("mute", &[(0.0, 0.0)])]);
    c.volume = 0.2;
    c.pan = 0.5;
    c.mute = true;
    project.channels.push(c);
    let n = neutral(&project).unwrap();
    let c = &n.channels[0];
    assert_eq!((c.volume, c.pan, c.mute), (1.0, 0.5, false));
}
