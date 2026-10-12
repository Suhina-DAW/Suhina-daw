//! Round trips of every part of the model, and files that are not projects or do not hold
//! together.

use super::*;

/// A project with every kind of thing the model holds: a group, an audio and a note track, a
/// return, sends, built-in and plug-in devices with state, shared and audio contents, loops,
/// fades, scenes with a tempo, stop buttons and extensions.
fn everything() -> Project {
    let ext =
        |key: &str| Extensions(vec![Extension { format: "als".into(), key: key.into(), data: vec![0, 1, 2, 255] }]);
    let clip = |content: u32, start: f64| Clip {
        content: ContentId(content),
        start,
        length: 4.0,
        offset: 0.5,
        loop_range: Some(LoopRange { start: 0.0, end: 2.0 }),
        enabled: true,
        fade_in: 0.25,
        fade_out: 1.0,
        extensions: Extensions::default(),
    };
    let channel = |id: u32, role: ChannelRole, output: Option<u32>| Channel {
        id: ChannelId(id),
        name: format!("Channel {id}"),
        role,
        volume: 0.5,
        pan: -0.25,
        mute: id == 2,
        solo: false,
        output: output.map(ChannelId),
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
    };
    let mut keys = channel(2, ChannelRole::Regular, Some(4));
    keys.sends.push(Send { target: ChannelId(3), level: 0.7, pre_fader: true });
    keys.devices = vec![
        Device {
            name: "FM".into(),
            kind: DeviceKind::Builtin("fm-synth".into()),
            enabled: true,
            state: None,
            params: vec![DeviceParam::new("volume", -6.0)],
            extensions: Extensions::default(),
        },
        Device {
            name: "Verb".into(),
            kind: DeviceKind::Vst3 { id: "0123456789ABCDEF0123456789ABCDEF".into() },
            enabled: false,
            state: Some(vec![9, 8, 7, 0, 255]),
            params: Vec::new(),
            extensions: ext("Vst3ControllerState"),
        },
        Device {
            name: "Old".into(),
            kind: DeviceKind::Vst2 { id: 1_234_567 },
            enabled: true,
            state: Some(Vec::new()),
            params: Vec::new(),
            extensions: Extensions::default(),
        },
        Device {
            name: "Clap".into(),
            kind: DeviceKind::Clap { id: "com.example.clap".into() },
            enabled: true,
            state: None,
            params: Vec::new(),
            extensions: Extensions::default(),
        },
        Device {
            name: "Glue".into(),
            kind: DeviceKind::Foreign { format: "als".into(), id: "GlueCompressor".into() },
            enabled: true,
            state: None,
            params: Vec::new(),
            extensions: ext("GlueCompressor"),
        },
    ];
    let track = |id: u32, notes: bool, channel: Option<u32>, parent: Option<u32>| Track {
        id: TrackId(id),
        name: format!("Track {id}"),
        color: Some(Rgb(0xD0, 0x8F, 0x6E)),
        content: ContentKinds { notes, audio: !notes },
        channel: channel.map(ChannelId),
        parent: parent.map(TrackId),
        extensions: Extensions::default(),
    };
    Project {
        transport: Transport { tempo_bpm: 128.5, time_signature: TimeSignature { numerator: 7, denominator: 8 } },
        tracks: vec![
            Track { color: None, ..track(1, false, Some(4), None) },
            track(2, true, Some(2), Some(1)),
            track(3, false, Some(5), Some(1)),
            Track { extensions: ext("ReturnTrack"), ..track(4, false, Some(3), None) },
        ],
        channels: vec![
            channel(1, ChannelRole::Master, None),
            keys,
            channel(3, ChannelRole::Effect, None),
            channel(4, ChannelRole::Submix, None),
            channel(5, ChannelRole::Regular, Some(4)),
        ],
        contents: vec![
            ClipContent {
                id: ContentId(1),
                name: "Chords".into(),
                length: 8.0,
                lanes: vec![ContentLane {
                    target: None,
                    data: LaneData::Notes(vec![
                        Note { time: 0.0, duration: 1.0, key: 60, velocity: 0.75, release_velocity: 0.5, channel: 0 },
                        Note { time: 1.5, duration: 0.25, key: 127, velocity: 1.0, release_velocity: 0.0, channel: 15 },
                    ]),
                }],
                extensions: ext("MidiClip"),
            },
            ClipContent {
                id: ContentId(2),
                name: "Loop".into(),
                length: 16.0,
                lanes: vec![ContentLane {
                    target: Some(TrackId(3)),
                    data: LaneData::Audio(AudioRegion {
                        file: "Samples/loop.wav".into(),
                        absolute_file: Some("C:/Music/Samples/loop.wav".into()),
                        sample_rate: Some(44_100),
                        channels: Some(2),
                        duration_seconds: Some(7.5),
                        warped: true,
                        stretch: StretchMode::HighQuality,
                        warp_markers: vec![
                            WarpMarker { time: 0.0, audio_time: 0.0 },
                            WarpMarker { time: 16.0, audio_time: 7.5 },
                        ],
                        gain: 0.8,
                        transpose: -1.5,
                    }),
                }],
                extensions: Extensions::default(),
            },
        ],
        arrangement: Arrangement {
            lanes: vec![
                ArrangementLane {
                    name: "Keys".into(),
                    track: Some(TrackId(2)),
                    clips: vec![clip(1, 0.0), clip(1, 8.0)],
                },
                ArrangementLane {
                    name: "Free".into(),
                    track: None,
                    clips: vec![Clip { enabled: false, loop_range: None, ..clip(2, 4.0) }],
                },
            ],
        },
        launcher: Launcher {
            scenes: vec![
                Scene { name: "Intro".into(), tempo_bpm: Some(100.0) },
                Scene { name: String::new(), tempo_bpm: None },
            ],
            columns: vec![LauncherColumn {
                track: TrackId(2),
                slots: vec![Slot { clip: Some(clip(1, 0.0)), has_stop: true }, Slot { clip: None, has_stop: false }],
            }],
        },
        extensions: ext("LiveSet"),
    }
}

#[test]
fn every_part_of_a_project_survives_a_round_trip() {
    let project = everything();
    assert_eq!(project.validate(), Ok(()));
    assert_eq!(from_json(&to_json(&project)), Ok(project));
}

#[test]
fn an_empty_project_survives_a_round_trip() {
    assert_eq!(from_json(&to_json(&Project::default())), Ok(Project::default()));
}

#[test]
fn files_are_written_and_read() {
    let dir = std::env::temp_dir().join(format!("loopwerk-locr-{}", std::process::id()));
    let path = dir.join("song.locr");
    assert_eq!(write_file(&path, &everything()), Ok(()));
    assert_eq!(read_file(&path), Ok(everything()));
    // Nothing is left beside it.
    let names: Vec<_> =
        std::fs::read_dir(&dir).map(|d| d.filter_map(Result::ok).map(|e| e.file_name()).collect()).unwrap_or_default();
    assert_eq!(names, ["song.locr"]);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_is_not_a_project_is_refused() {
    let refused = |text: &str| from_json(text).is_err();
    assert!(refused("not json"));
    assert!(refused("[1, 2]"));
    assert!(refused(r#"{ "version": 1, "project": {} }"#), "no format");
    assert!(refused(r#"{ "format": "als", "version": 1, "project": {} }"#));
    assert!(refused(r#"{ "format": "locr", "project": {} }"#), "no version");
    assert!(refused(r#"{ "format": "locr", "version": 2, "project": {} }"#), "a newer version");
    assert!(refused(r#"{ "format": "locr", "version": 1 }"#), "no project");
    assert_eq!(from_json(r#"{ "format": "locr", "version": 1, "project": {} }"#), Ok(Project::default()));
}

#[test]
fn hostile_values_are_errors_not_crashes() {
    let with = |project: &str| from_json(&format!(r#"{{ "format": "locr", "version": 1, "project": {project} }}"#));
    for bad in [
        r#"{ "tracks": [ { "id": -1 } ] }"#,
        r#"{ "tracks": [ { "id": 99999999999 } ] }"#,
        r#"{ "tracks": [ { "id": 1, "color": "red" } ] }"#,
        r#"{ "tracks": "many" }"#,
        r#"{ "transport": { "tempo_bpm": 0 } }"#,
        r#"{ "transport": { "time_signature": [0, 4] } }"#,
        r#"{ "transport": { "time_signature": [300, 4] } }"#,
        r#"{ "channels": [ { "id": 1, "role": "boss" } ] }"#,
        r#"{ "channels": [ { "id": 1, "pan": 3 } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "vst9" } } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "vst3", "id": "x" }, "state": "***" } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { "notes": [ { "time": 0, "duration": 1, "key": 200 } ] } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": "long" } ] }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "loop": [3, 1] } ] } ] } }"#,
    ] {
        assert!(with(bad).is_err(), "{bad} was read");
    }
}

#[test]
fn a_project_that_does_not_hold_together_is_refused() {
    let mut project = everything();
    project.tracks.push(project.tracks[0].clone());
    let error = from_json(&to_json(&project)).err().map(|e| e.0).unwrap_or_default();
    assert!(error.contains("used twice"), "{error}");
}

#[test]
fn numbers_that_are_not_finite_are_written_as_zero() {
    let mut project = everything();
    project.channels[0].volume = f64::NAN;
    let read = from_json(&to_json(&project)).map(|p| p.channels[0].volume);
    assert_eq!(read, Ok(0.0));
}

#[test]
fn audio_paths_follow_a_project_saved_elsewhere() {
    let base = std::env::temp_dir().join(format!("loopwerk-locr-rebase-{}", std::process::id()));
    let samples = base.join("old").join("Samples");
    std::fs::create_dir_all(&samples).unwrap_or_default();
    std::fs::write(samples.join("loop.wav"), b"RIFF").unwrap_or_default();
    let mut project = everything();
    rebase_audio(&mut project, &base.join("old"), &base.join("new").join("songs"));
    let LaneData::Audio(region) = &project.contents[1].lanes[0].data else { return };
    assert_eq!(region.file, "../../old/Samples/loop.wav");
    let absolute = region.absolute_file.clone().unwrap_or_default();
    assert!(Path::new(&absolute).ends_with("old/Samples/loop.wav"), "{absolute}");
    // Saved beside it again: the path is relative to the same folder.
    rebase_audio(&mut project, &base.join("new").join("songs"), &base.join("old"));
    let LaneData::Audio(region) = &project.contents[1].lanes[0].data else { return };
    assert_eq!(region.file, "Samples/loop.wav");
    let _clean = std::fs::remove_dir_all(&base);
}

#[test]
fn a_missing_audio_file_is_found_by_its_absolute_path() {
    let base = std::env::temp_dir().join(format!("loopwerk-locr-absolute-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap_or_default();
    let file = base.join("kept.wav");
    std::fs::write(&file, b"RIFF").unwrap_or_default();
    let mut project = everything();
    if let LaneData::Audio(region) = &mut project.contents[1].lanes[0].data {
        region.file = "gone/kept.wav".into();
        region.absolute_file = Some(file.display().to_string());
    }
    rebase_audio(&mut project, &base.join("elsewhere"), &base);
    let LaneData::Audio(region) = &project.contents[1].lanes[0].data else { return };
    assert_eq!(region.file, "kept.wav");
    let _clean = std::fs::remove_dir_all(&base);
}
