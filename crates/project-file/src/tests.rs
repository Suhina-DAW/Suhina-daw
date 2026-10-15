//! Round trips of every part of the model, and files that are not projects or do not hold
//! together.

use super::*;

/// A project with every kind of thing the model holds: a group, an audio and a note track, a
/// return, sends, built-in and plug-in devices with state, shared and audio contents, loops,
/// fades, a follow action, scenes with a tempo, stop buttons and extensions.
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
        fade_in_curve: FadeCurve { shape: FadeShape::SCurve, curve: -0.5 },
        fade_out_curve: FadeCurve { shape: FadeShape::EqualPower, curve: 0.75 },
        follow_action: Default::default(),
        extensions: Extensions::default(),
    };
    let follow = FollowAction {
        enabled: true,
        time: 6.0,
        linked: false,
        action_a: FollowKind::Jump(1),
        action_b: FollowKind::Other,
        chance_a: 30,
        chance_b: 70,
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
        automation: Vec::new(),
    };
    let mut keys = channel(2, ChannelRole::Regular, Some(4));
    keys.sends.push(Send { target: ChannelId(3), level: 0.7, pre_fader: true });
    keys.automation = vec![
        AutomationLane {
            param: "volume".into(),
            points: vec![AutomationPoint { beat: 0.0, value: 0.0, curve: 0.5 }, AutomationPoint::new(8.0, 1.25)],
            enabled: true,
        },
        AutomationLane { param: "mute".into(), points: vec![AutomationPoint::new(2.0, 1.0)], enabled: false },
    ];
    keys.devices = vec![
        Device {
            name: "FM".into(),
            kind: DeviceKind::Builtin("fm-synth".into()),
            enabled: true,
            state: None,
            params: vec![DeviceParam::new("volume", -6.0)],
            automation: vec![AutomationLane {
                param: "volume".into(),
                points: vec![AutomationPoint { beat: 1.0, value: -12.0, curve: -0.75 }, AutomationPoint::new(3.5, 0.0)],
                enabled: false,
            }],
            extensions: Extensions::default(),
        },
        Device {
            name: "Verb".into(),
            kind: DeviceKind::Vst3 { id: "0123456789ABCDEF0123456789ABCDEF".into() },
            enabled: false,
            state: Some(vec![9, 8, 7, 0, 255]),
            params: Vec::new(),
            automation: Vec::new(),
            extensions: ext("Vst3ControllerState"),
        },
        Device {
            name: "Old".into(),
            kind: DeviceKind::Vst2 { id: 1_234_567 },
            enabled: true,
            state: Some(Vec::new()),
            params: Vec::new(),
            automation: Vec::new(),
            extensions: Extensions::default(),
        },
        Device {
            name: "Clap".into(),
            kind: DeviceKind::Clap { id: "com.example.clap".into() },
            enabled: true,
            state: None,
            params: vec![DeviceParam::new("3", 0.25)],
            automation: vec![AutomationLane {
                param: "3".into(),
                points: vec![
                    AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 },
                    AutomationPoint { beat: 4.0, value: 1.0, curve: 0.0 },
                    AutomationPoint { beat: 4.0, value: 0.5, curve: 0.0 },
                ],
                enabled: true,
            }],
            extensions: Extensions::default(),
        },
        Device {
            name: "Glue".into(),
            kind: DeviceKind::Foreign { format: "als".into(), id: "GlueCompressor".into() },
            enabled: true,
            state: None,
            params: Vec::new(),
            automation: Vec::new(),
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
                        stream: Some(3),
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
                slots: vec![
                    Slot { clip: Some(Clip { follow_action: follow, ..clip(1, 0.0) }), has_stop: true },
                    Slot { clip: None, has_stop: false },
                ],
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
    let dir = std::env::temp_dir().join(format!("suhina-project-file-{}", std::process::id()));
    let path = dir.join("song.suhina");
    assert_eq!(write_file(&path, &everything()), Ok(()));
    assert_eq!(read_file(&path), Ok(everything()));
    // Nothing is left beside it.
    let names: Vec<_> =
        std::fs::read_dir(&dir).map(|d| d.filter_map(Result::ok).map(|e| e.file_name()).collect()).unwrap_or_default();
    assert_eq!(names, ["song.suhina"]);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_names_the_suhina_format() {
    let text = to_json(&Project::default());
    let root = serde_json::from_str::<Value>(&text).unwrap_or_default();
    assert_eq!(root.get("format").and_then(Value::as_str), Some("suhina"));
    assert_eq!(root.get("version").and_then(Value::as_u64), Some(VERSION));
}

#[test]
fn a_file_saved_before_the_rename_still_opens() {
    // Written with the old format name, as projects saved before October 2026 are.
    let old = to_json(&everything()).replacen(r#""format": "suhina""#, r#""format": "locr""#, 1);
    assert!(old.contains(r#""format": "locr""#), "{old}");
    assert_eq!(from_json(&old), Ok(everything()));
    // And from a file with the old extension.
    let dir = std::env::temp_dir().join(format!("suhina-project-file-old-{}", std::process::id()));
    let path = dir.join("song.locr");
    let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &old));
    assert!(written.is_ok(), "{written:?}");
    assert_eq!(read_file(&path), Ok(everything()));
    // Saved again, it names the new format.
    assert!(to_json(&everything()).contains(r#""format": "suhina""#));
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_files_are_known_by_either_extension() {
    for name in ["a.suhina", "a.SUHINA", "a.locr", "a.LOCR", "dir/b c.suhina"] {
        assert!(is_project_file(Path::new(name)), "{name}");
    }
    for name in ["a.als", "a.json", "a", "suhina", "a.suhina.saving"] {
        assert!(!is_project_file(Path::new(name)), "{name}");
    }
}

#[test]
fn what_is_not_a_project_is_refused() {
    let refused = |text: &str| from_json(text).is_err();
    assert!(refused("not json"));
    assert!(refused("[1, 2]"));
    assert!(refused(r#"{ "version": 1, "project": {} }"#), "no format");
    assert!(refused(r#"{ "format": "als", "version": 1, "project": {} }"#));
    assert!(refused(r#"{ "format": "suhina", "project": {} }"#), "no version");
    assert!(refused(r#"{ "format": "suhina", "version": 2, "project": {} }"#), "a newer version");
    assert!(refused(r#"{ "format": "suhina", "version": 1 }"#), "no project");
    assert!(refused(r#"{ "format": "locr", "version": 2, "project": {} }"#), "a newer version, old name");
    assert_eq!(from_json(r#"{ "format": "suhina", "version": 1, "project": {} }"#), Ok(Project::default()));
    assert_eq!(from_json(r#"{ "format": "locr", "version": 1, "project": {} }"#), Ok(Project::default()));
}

#[test]
fn hostile_values_are_errors_not_crashes() {
    let with = |project: &str| from_json(&format!(r#"{{ "format": "suhina", "version": 1, "project": {project} }}"#));
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
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" }, "automation": [ { "points": [] } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" }, "automation": [ { "param": "1", "points": [ { "beat": "one", "value": 1 } ] } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" }, "automation": 5 } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": [ { "param": "pan", "points": [ { "beat": 0, "value": 0, "curve": 1.5 } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": [ { "param": "pan", "points": [ { "beat": 0, "value": 0, "curve": "bent" } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": [ { "param": "pan", "enabled": "yes", "points": [] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": { "param": "pan" } } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "vst3", "id": "x" }, "state": "***" } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { "notes": [ { "time": 0, "duration": 1, "key": 200 } ] } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": "long" } ] }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "loop": [3, 1] } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_in_shape": "wobbly" } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_out_shape": 3 } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_in_curve": 2 } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": true } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "time": 0 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "time": 1e9 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "a": "up" } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "b": "jump 0" } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "chance_a": 101 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "chance_b": -3 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "enabled": "on" } } ] } ] } }"#,
    ] {
        assert!(with(bad).is_err(), "{bad} was read");
    }
}

/// A lane's points are read in beat order, points at one beat in the order written.
#[test]
fn automation_points_are_read_in_beat_order() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" },
            "automation": [ { "param": "2", "points": [ { "beat": 4, "value": 1 }, { "beat": 0, "value": 0 }, { "beat": 4, "value": 0.5 } ] } ] } ] } ] } }"#,
    )
    .unwrap();
    let points = &project.channels[0].devices[0].automation[0].points;
    let read: Vec<(f64, f64)> = points.iter().map(|p| (p.beat, p.value)).collect();
    assert_eq!(read, [(0.0, 0.0), (4.0, 1.0), (4.0, 0.5)]);
}

/// A curve and an enabled flag are written only when they are not the default, so a file
/// with straight lanes that play keeps its shape; missing, they read as a straight line that
/// plays. A channel without lanes writes none.
#[test]
fn automation_defaults_are_left_out_and_read_back() {
    let lane = AutomationLane { param: "pan".into(), points: vec![AutomationPoint::new(0.0, 0.5)], enabled: true };
    let mut project =
        from_json(r#"{ "format": "suhina", "version": 1, "project": { "channels": [ { "id": 1 } ] } }"#).unwrap();
    assert!(project.channels[0].automation.is_empty());
    assert!(!to_json(&project).contains("automation"));
    project.channels[0].automation = vec![lane.clone()];
    let json = to_json(&project);
    assert!(json.contains("automation") && !json.contains("curve") && !json.contains("\"enabled\": false"), "{json}");
    assert_eq!(from_json(&json).unwrap().channels[0].automation, [lane]);
}

/// A clip written before fade shapes existed has linear, unbent fades, so it sounds the same.
#[test]
fn a_clip_without_fade_shapes_has_linear_fades() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "contents": [ { "id": 1, "length": 4 } ],
            "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_in": 1, "fade_out": 1 } ] } ] } } }"#,
    )
    .unwrap();
    let clip = &project.arrangement.lanes[0].clips[0];
    assert_eq!((clip.fade_in_curve, clip.fade_out_curve), (FadeCurve::default(), FadeCurve::default()));
    assert_eq!(FadeCurve::default(), FadeCurve { shape: FadeShape::Linear, curve: 0.0 });
}

/// A clip without a follow action writes none, and a file written before follow actions reads
/// as off.
#[test]
fn a_clip_without_a_follow_action_has_it_off() {
    let mut project = everything();
    if let Some(clip) = project.launcher.columns[0].slots[0].clip.as_mut() {
        clip.follow_action = FollowAction::default();
    }
    let text = to_json(&project);
    assert!(!text.contains("\"follow\""), "{text}");
    let read = from_json(&text).unwrap();
    assert_eq!(read.launcher.columns[0].slots[0].clip.as_ref().unwrap().follow_action, FollowAction::default());
    // Only what is given changes from the default.
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "contents": [ { "id": 1, "length": 4 } ],
            "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "enabled": true, "b": "STOP" } } ] } ] } } }"#,
    )
    .unwrap();
    let follow = project.arrangement.lanes[0].clips[0].follow_action;
    assert_eq!(follow, FollowAction { enabled: true, action_b: FollowKind::Stop, ..FollowAction::default() });
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
    let base = std::env::temp_dir().join(format!("suhina-project-file-rebase-{}", std::process::id()));
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
    let base = std::env::temp_dir().join(format!("suhina-project-file-absolute-{}", std::process::id()));
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
