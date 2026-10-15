//! Group buses: a group with devices processes the sum of the tracks routed into it.

use suhina_model::*;

use crate::chain_tests::{SR, blocks, device, left, max_diff, note, peak};
use crate::{Renderer, render_offline};

/// Tracks 1 and 2 each play an A4 for a beat into group track 3 (channel 3), whose channel
/// holds `group_devices`; track 4 plays the same note straight to the output, unless `outside`
/// is false.
fn grouped(group_devices: Vec<Device>, outside: bool) -> Project {
    let channel = |id: u32, output: Option<u32>, devices: Vec<Device>| Channel {
        id: ChannelId(id),
        name: format!("C{id}"),
        role: ChannelRole::Regular,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        output: output.map(ChannelId),
        sends: Vec::new(),
        devices,
        extensions: Extensions::default(),
    };
    let track = |id: u32, parent: Option<u32>| Track {
        id: TrackId(id),
        name: format!("T{id}"),
        color: None,
        content: ContentKinds { notes: parent.is_some() || id == 4, audio: false },
        channel: Some(ChannelId(id)),
        parent: parent.map(TrackId),
        extensions: Extensions::default(),
    };
    let clip = Clip {
        content: ContentId(1),
        start: 0.0,
        length: 1.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        extensions: Extensions::default(),
    };
    let players: Vec<u32> = if outside { vec![1, 2, 4] } else { vec![1, 2] };
    let mut tracks = vec![track(1, Some(3)), track(2, Some(3)), track(3, None)];
    let mut channels = vec![channel(1, Some(3), vec![]), channel(2, Some(3), vec![]), channel(3, None, group_devices)];
    if outside {
        tracks.push(track(4, None));
        channels.push(channel(4, None, vec![]));
    }
    Project {
        tracks,
        channels,
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(0.0, 1.0, 69)]) }],
            extensions: Extensions::default(),
        }],
        arrangement: Arrangement {
            lanes: players
                .iter()
                .map(|&t| ArrangementLane { name: format!("l{t}"), track: Some(TrackId(t)), clips: vec![clip.clone()] })
                .collect(),
        },
        ..Project::default()
    }
}

fn first_sound(out: &[f32]) -> usize {
    left(out).iter().position(|s| s.abs() > 1e-6).unwrap_or(usize::MAX)
}

fn db(value: f64) -> f32 {
    10f64.powf(value / 20.0) as f32
}

#[test]
fn a_limiter_on_a_group_limits_the_sum_of_its_tracks() {
    let plain = render_offline(&grouped(vec![], false), SR, 512).unwrap();
    // Two tracks of one note in phase: twice one track's level.
    let single = peak(&plain) / 2.0;
    let ceiling = (single * 1.2).log10() * 20.0;
    let limited = render_offline(
        &grouped(vec![device("limiter", &[("threshold", f64::from(ceiling)), ("ceiling", f64::from(ceiling))])], false),
        SR,
        512,
    )
    .unwrap();
    // Each track alone is under the ceiling; their sum is not, and the bus brings it down to it.
    // Limiting each track on its own would have left the sum at twice the single level.
    let reached = peak(&limited);
    assert!(reached <= db(f64::from(ceiling)) * 1.001, "{reached} over {}", db(f64::from(ceiling)));
    assert!(reached > single, "{reached}");
    assert!(peak(&plain) > reached * 1.5);
}

#[test]
fn a_group_without_devices_sounds_as_before_and_a_neutral_bus_changes_nothing() {
    let plain = render_offline(&grouped(vec![], true), SR, 512).unwrap();
    let neutral = render_offline(&grouped(vec![device("utility", &[])], true), SR, 512).unwrap();
    // The bus's chain adds a tail to the render, nothing else.
    assert!(neutral.len() > plain.len());
    assert!(max_diff(&neutral[..plain.len()], &plain) < 1e-5, "{}", max_diff(&neutral[..plain.len()], &plain));
    assert!(peak(&neutral[plain.len()..]) < 1e-6);
}

#[test]
fn the_group_volume_applies_after_its_devices_and_the_tracks_volume_before() {
    let plain = render_offline(&grouped(vec![], false), SR, 512).unwrap();
    let mut project = grouped(vec![device("utility", &[("gain", -6.0206)])], false);
    project.channels[2].volume = 0.5;
    project.channels[0].volume = 0.0;
    let out = render_offline(&project, SR, 512).unwrap();
    // One of two tracks left, halved by the device and halved again by the group's fader.
    let expected: Vec<f32> = plain.iter().map(|s| s * 0.125).collect();
    assert!(max_diff(&out[..plain.len()], &expected) < 1e-4, "{}", max_diff(&out[..plain.len()], &expected));
}

#[test]
fn latency_on_a_bus_and_in_it_is_compensated_on_every_path() {
    let reference = first_sound(&render_offline(&grouped(vec![], true), SR, 512).unwrap());
    // A limiter (lookahead) on the group, another on track 1 inside it: track 1 is delayed by
    // both, track 2 by the group's, track 4 by neither; all must still start together.
    let mut project = grouped(vec![device("limiter", &[])], true);
    project.channels[0].devices.push(device("limiter", &[]));
    let out = render_offline(&project, SR, 512).unwrap();
    let renderer = Renderer::new(&project, SR).unwrap();
    let lookahead = renderer.latency() / 2;
    assert!(lookahead > 0);
    assert_eq!(first_sound(&out), reference + renderer.latency() as usize);

    // Each track heard alone (the others at zero volume, so the routing stays) starts at the
    // same frame as all together.
    for alone in [0usize, 1, 3] {
        let mut one = project.clone();
        for (i, channel) in one.channels.iter_mut().enumerate() {
            if i != alone && i != 2 {
                channel.volume = 0.0;
            }
        }
        let out = render_offline(&one, SR, 512).unwrap();
        assert_eq!(first_sound(&out), reference + renderer.latency() as usize, "track index {alone}");
    }
}

#[test]
fn nested_groups_and_muting() {
    // Group 3 plays into group 5, which has a device; group 3 has one too.
    let mut project = grouped(vec![device("utility", &[("gain", -6.0206)])], false);
    project.channels[2].output = Some(ChannelId(5));
    project.tracks.push(Track {
        id: TrackId(5),
        name: "G2".into(),
        parent: None,
        channel: Some(ChannelId(5)),
        ..project.tracks[2].clone()
    });
    project.tracks[2].parent = Some(TrackId(5));
    project.channels.push(Channel {
        id: ChannelId(5),
        output: None,
        devices: vec![device("utility", &[("gain", -6.0206)])],
        ..project.channels[2].clone()
    });
    let plain = render_offline(&grouped(vec![], false), SR, 512).unwrap();
    let out = render_offline(&project, SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 0.25).collect();
    assert!(max_diff(&out[..plain.len()], &expected) < 1e-4, "{}", max_diff(&out[..plain.len()], &expected));

    project.channels[3].mute = true;
    assert_eq!(render_offline(&project, SR, 512), Ok(Vec::new()));
}

#[test]
fn a_bus_does_not_depend_on_the_block_size() {
    let mut project = grouped(vec![device("limiter", &[("threshold", -12.0)])], true);
    project.channels[1].devices.push(device("utility", &[("gain", 3.0)]));
    let reference = render_offline(&project, SR, 1024).unwrap();
    for size in [1usize, 37, 256, 4096] {
        let out = render_offline(&project, SR, size).unwrap();
        assert!(max_diff(&out, &reference) < 1e-6, "block {size}: {}", max_diff(&out, &reference));
    }
    // And played live, block by block.
    let mut renderer = Renderer::new(&project, SR).unwrap();
    let live = blocks(&mut renderer, reference.len() / 2 / 300 + 1, 300);
    assert!(max_diff(&live[..reference.len()], &reference) < 1e-6);
}

/// A device that follows the song position (auto-pan synced to the beat) gets the block's own
/// transport wherever it is: on a track that plays into a group, on one that plays straight to
/// the output (both run in the stages plug-ins share) and on the group's bus (run after them).
/// Given the previous block's, its LFO would jump back a block at every block boundary, and the
/// render would depend on the block size.
#[test]
fn a_synced_device_gets_its_blocks_own_transport_on_a_track_and_on_a_bus() {
    let synced = || device("auto-pan", &[("sync", 1.0), ("amount", 1.0)]);
    for place in ["track in the group", "track to the output", "group"] {
        let mut project = grouped(vec![], true);
        match place {
            "track in the group" => project.channels[0].devices.push(synced()),
            "track to the output" => project.channels[3].devices.push(synced()),
            _ => project.channels[2].devices.push(synced()),
        }
        let reference = render_offline(&project, SR, 1024).unwrap();
        for size in [1usize, 256, 333, 4096] {
            let out = render_offline(&project, SR, size).unwrap();
            assert!(max_diff(&out, &reference) < 1e-6, "{place}, block {size}: {}", max_diff(&out, &reference));
        }
    }
}

#[test]
fn a_routing_cycle_through_buses_is_silence_not_a_hang() {
    let mut project = grouped(vec![device("utility", &[])], false);
    project.channels[2].output = Some(ChannelId(1));
    assert_eq!(render_offline(&project, SR, 512), Ok(Vec::new()));
}
