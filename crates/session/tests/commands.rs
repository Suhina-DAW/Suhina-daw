use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{CommandKind, Session, commands};

fn project() -> Project {
    let track = |id: u32, name: &str| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        parent: None,
        extensions: Extensions::default(),
    };
    let channel = |id: u32| Channel {
        id: ChannelId(id),
        name: format!("C{id}"),
        role: ChannelRole::Regular,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
    };
    let clip = |start: f64| Clip {
        content: ContentId(1),
        start,
        length: 4.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        extensions: Extensions::default(),
    };
    Project {
        tracks: vec![track(1, "Bass"), track(2, "Lead")],
        channels: vec![channel(1), channel(2)],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "Riff".into(),
            length: 4.0,
            lanes: vec![ContentLane {
                target: None,
                data: LaneData::Notes(vec![Note {
                    time: 0.0,
                    duration: 1.0,
                    key: 36,
                    velocity: 1.0,
                    release_velocity: 0.5,
                    channel: 0,
                }]),
            }],
            extensions: Extensions::default(),
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                name: "Bass".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(0.0), clip(8.0)],
            }],
        },
        ..Project::default()
    }
}

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

#[test]
fn every_command_is_listed_with_a_unique_id() {
    let mut session = Session::new(project());
    let listed = run(&mut session, "commands.list", json!({})).unwrap();
    let ids: Vec<&str> = listed.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), commands().len());
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    assert!(ids.iter().all(|id| id.contains('.')));
}

#[test]
fn reads_project_tracks_and_clips() {
    let mut session = Session::new(project());
    let info = run(&mut session, "project.info", json!({})).unwrap();
    assert_eq!(info["tracks"], 2);
    assert_eq!(info["arrangement_clips"], 2);
    assert_eq!(info["can_undo"], false);

    let tracks = run(&mut session, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["name"], "Bass");
    assert_eq!(tracks[0]["volume_db"], 0.0);

    let clips = run(&mut session, "project.clips", json!({ "track": "bass" })).unwrap();
    assert_eq!(clips.as_array().unwrap().len(), 2);
    assert_eq!(clips[1]["start"], 8.0);
    assert_eq!(clips[1]["notes"], 1);
}

#[test]
fn edits_record_one_undo_step_each() {
    let mut session = Session::new(project());
    run(&mut session, "transport.setTempo", json!({ "bpm": 128 })).unwrap();
    run(&mut session, "track.setVolume", json!({ "track": "Lead", "db": -6 })).unwrap();
    run(&mut session, "track.setPan", json!({ "track": 2, "pan": -1 })).unwrap();
    run(&mut session, "track.setMute", json!({ "track": "Bass", "mute": true })).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 1, "start": 4 })).unwrap();

    let p = session.project();
    assert_eq!(p.transport.tempo_bpm, 128.0);
    assert!((p.channels[1].volume - 0.501).abs() < 1e-3);
    assert_eq!(p.channels[1].pan, -1.0);
    assert!(p.channels[0].mute);
    assert_eq!(p.arrangement.lanes[0].clips[1].start, 4.0);

    for _ in 0..5 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
    assert!(run(&mut session, "edit.undo", json!({})).is_err());

    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(session.project().transport.tempo_bpm, 128.0);
}

#[test]
fn a_new_edit_clears_redo_and_no_op_edits_record_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "transport.setTempo", json!({ "bpm": 100 })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "transport.setTempo", json!({ "bpm": 90 })).unwrap();
    assert!(run(&mut session, "edit.redo", json!({})).is_err());

    // Setting the same value again changes nothing, so there is nothing new to undo.
    run(&mut session, "transport.setTempo", json!({ "bpm": 90 })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().transport.tempo_bpm, 120.0);
}

#[test]
fn volume_accepts_silence() {
    let mut session = Session::new(project());
    let result = run(&mut session, "track.setVolume", json!({ "track": "Bass", "db": "-inf" })).unwrap();
    assert_eq!(result["volume_db"], Value::Null);
    assert_eq!(session.project().channels[0].volume, 0.0);
}

/// Every editing command, given hostile parameters, returns an error and leaves the project as
/// it was.
#[test]
fn bad_parameters_never_change_the_project() {
    let hostile = [
        json!(null),
        json!({}),
        json!({ "track": "nope" }),
        json!({ "track": 99, "bpm": 120, "db": 0, "pan": 0, "mute": true, "index": 0, "start": 0 }),
        json!({ "track": -1 }),
        json!({ "track": 1.5 }),
        json!({ "track": "Bass", "bpm": "fast", "db": 99, "pan": 2, "mute": "yes", "index": 7, "start": -1 }),
        json!({ "track": "Bass", "bpm": 1e308, "db": f64::MAX, "pan": -2, "mute": 1, "index": -1, "start": f64::MAX }),
        json!({ "track": [1], "index": u64::MAX }),
    ];
    for spec in commands().iter().filter(|c| c.kind == CommandKind::Edit) {
        for params in &hostile {
            // Keep only this command's own keys, so the values themselves are what gets tested.
            let params = match params.as_object() {
                Some(object) => Value::Object(
                    object
                        .iter()
                        .filter(|(k, _)| spec.params.iter().any(|p| p.name == k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                None => params.clone(),
            };
            let mut session = Session::new(project());
            let result = session.execute(spec.id, &params);
            if let Err(e) = result {
                assert!(!e.0.is_empty());
                assert_eq!(session.project(), &project(), "{} with {params} changed the project", spec.id);
            }
        }
    }
}

#[test]
fn unknown_commands_and_non_object_params_are_errors() {
    let mut session = Session::new(project());
    let err = run(&mut session, "track.setVolumee", json!({})).unwrap_err();
    assert!(err.contains("track.setVolume"), "{err}");
    assert!(run(&mut session, "nope", json!({})).is_err());
    assert!(run(&mut session, "project.info", json!([1, 2])).is_err());
    let err = run(&mut session, "transport.setTempo", json!({ "bpm": 100, "temp": 1 })).unwrap_err();
    assert!(err.contains("\"temp\""), "{err}");
    assert_eq!(session.project().transport.tempo_bpm, 120.0);
}

#[test]
fn a_clip_moves_to_another_track_that_takes_its_kind() {
    let mut session = Session::new(project());
    let moved =
        run(&mut session, "clip.move", json!({ "track": "Bass", "index": 1, "start": 2, "to_track": "Lead" })).unwrap();
    assert_eq!(moved["track"], 2);
    let p = session.project();
    assert_eq!(p.arrangement.lanes[0].clips.len(), 1);
    let lead = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    assert!(lead.clips.iter().any(|c| c.start == 2.0));
    assert_eq!(moved["index"], lead.clips.iter().position(|c| c.start == 2.0).unwrap());

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &project());
    assert!(
        run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 0, "to_track": "Nobody" }))
            .is_err()
    );
    assert!(run(&mut session, "clip.move", json!({ "track": "Bass", "index": 9, "start": 0, "to_track": 2 })).is_err());
}

#[test]
fn solo_is_an_edit_and_can_be_exclusive() {
    let mut session = Session::new(project());
    run(&mut session, "track.setSolo", json!({ "track": "Bass", "solo": true })).unwrap();
    run(&mut session, "track.setSolo", json!({ "track": "Lead", "solo": true })).unwrap();
    let solos = |s: &Session| s.project().channels.iter().map(|c| c.solo).collect::<Vec<_>>();
    assert_eq!(solos(&session), [true, true]);

    run(&mut session, "track.setSolo", json!({ "track": "Lead", "solo": true, "exclusive": true })).unwrap();
    assert_eq!(solos(&session), [false, true]);

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(solos(&session), [true, true]);
    assert!(run(&mut session, "track.setSolo", json!({ "track": "Bass", "solo": "yes" })).is_err());
}

#[test]
fn a_track_routes_into_a_group_and_back_to_main() {
    let mut p = project();
    p.tracks.push(Track { id: TrackId(3), name: "Group".into(), channel: Some(ChannelId(3)), ..p.tracks[0].clone() });
    p.channels.push(Channel {
        id: ChannelId(3),
        name: "Group".into(),
        role: ChannelRole::Submix,
        ..p.channels[0].clone()
    });
    let mut session = Session::new(p);

    assert_eq!(run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Group" })).unwrap()["to"], "Group");
    assert_eq!(session.project().channels[0].output, Some(ChannelId(3)));
    // Not into a track that is not a group, and not into a loop.
    assert!(run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Lead" })).is_err());
    assert!(run(&mut session, "track.setOutput", json!({ "track": "Group", "to": "Group" })).is_err());

    run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "main" })).unwrap();
    assert_eq!(session.project().channels[0].output, None);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].output, Some(ChannelId(3)));
}

/// One lane with three one-beat-long clips at 0, 4 and 8 and a looping eight-beat clip at 12.
fn lane_project() -> Project {
    let mut p = project();
    let clip = |start: f64, length: f64| Clip { start, length, ..p.arrangement.lanes[0].clips[0].clone() };
    let mut looping = clip(12.0, 8.0);
    looping.loop_range = Some(LoopRange { start: 0.0, end: 4.0 });
    looping.offset = 0.0;
    p.arrangement.lanes[0].clips = vec![clip(0.0, 4.0), clip(4.0, 4.0), clip(8.0, 4.0), looping];
    p
}

fn spans(session: &Session) -> Vec<(f64, f64, f64)> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length, c.offset)).collect()
}

#[test]
fn a_moved_clip_replaces_what_it_covers() {
    let mut session = Session::new(lane_project());
    // The clip at 0 moves to 6: the clip at 4 is shortened, the one at 8 starts later.
    let moved = run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 6 })).unwrap();
    assert_eq!(spans(&session)[..3], [(4.0, 2.0, 0.0), (6.0, 4.0, 0.0), (10.0, 2.0, 2.0)]);
    assert_eq!(moved["index"], 1);

    // Into the middle of the looping clip: it splits, the second part keeps the loop's phase.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 13 })).unwrap();
    let after: Vec<_> = spans(&session).into_iter().filter(|s| s.0 >= 12.0).collect();
    assert_eq!(after, [(12.0, 1.0, 0.0), (13.0, 4.0, 0.0), (17.0, 3.0, 1.0)]);

    // A clip entirely covered disappears.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 3, "start": 4 })).unwrap();
    assert_eq!(spans(&session), [(0.0, 4.0, 0.0), (4.0, 8.0, 0.0)]);
}

#[test]
fn a_stacked_clip_leaves_the_others_alone() {
    let mut session = Session::new(lane_project());
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 6, "stack": true })).unwrap();
    assert_eq!(spans(&session).len(), 4);
    assert!(spans(&session).contains(&(4.0, 4.0, 0.0)));
    assert!(spans(&session).contains(&(6.0, 4.0, 0.0)));
    assert!(
        run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 1, "stack": "yes" })).is_err()
    );
}

#[test]
fn a_stacked_paste_or_duplicate_leaves_the_others_alone() {
    // Without stack, a pasted copy of the clip at 0 placed at 6 shortens the clip at 4.
    let mut session = Session::new(lane_project());
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 0 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 6 })).unwrap();
    assert!(spans(&session).contains(&(4.0, 2.0, 0.0)), "{:?}", spans(&session));

    // With stack, every clip keeps its span and the copy lies over them.
    let mut session = Session::new(lane_project());
    let before = spans(&session);
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 0 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 6, "stack": true })).unwrap();
    assert_eq!(spans(&session).len(), before.len() + 1);
    assert!(before.iter().all(|s| spans(&session).contains(s)), "{:?}", spans(&session));
    assert!(spans(&session).contains(&(6.0, 4.0, 0.0)));
    assert!(run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 6, "stack": 1 })).is_err());

    // A duplicate replaces the clip after it unless stacked.
    let mut session = Session::new(lane_project());
    run(&mut session, "clip.duplicate", json!({ "track": "Bass", "index": 0 })).unwrap();
    assert_eq!(spans(&session).len(), 4, "the copy replaces the clip at 4");
    let mut session = Session::new(lane_project());
    let before = spans(&session);
    let copy = run(&mut session, "clip.duplicate", json!({ "track": "Bass", "index": 0, "stack": true })).unwrap();
    assert_eq!(copy["start"], 4.0);
    assert_eq!(spans(&session).len(), before.len() + 1);
    assert!(before.iter().all(|s| spans(&session).contains(s)), "{:?}", spans(&session));
    assert!(run(&mut session, "clip.duplicate", json!({ "track": "Bass", "index": 0, "stack": "yes" })).is_err());
}

fn order(session: &Session) -> Vec<String> {
    session.project().tracks.iter().map(|t| t.name.clone()).collect()
}

#[test]
fn tracks_group_ungroup_and_move() {
    let mut session = Session::new(project());
    let group = run(&mut session, "track.group", json!({ "tracks": ["Lead", 1], "name": "Band" })).unwrap();
    let id = TrackId(u32::try_from(group["track"].as_u64().unwrap()).unwrap());
    assert_eq!(order(&session), ["Band", "Bass", "Lead"]);
    let p = session.project();
    let group_channel = p.tracks[0].channel.unwrap();
    assert!(p.tracks[1..].iter().all(|t| t.parent == Some(id)));
    assert!(p.channels.iter().filter(|c| c.id != group_channel).all(|c| c.output == Some(group_channel)));
    assert_eq!(p.validate(), Ok(()));

    // A group moves with its tracks.
    run(&mut session, "track.move", json!({ "track": "Lead", "to": 0 })).unwrap();
    assert_eq!(order(&session), ["Lead", "Band", "Bass"]);
    run(&mut session, "track.move", json!({ "track": "Band", "to": 9 })).unwrap();
    assert_eq!(order(&session), ["Lead", "Band", "Bass"]);

    run(&mut session, "track.ungroup", json!({ "track": "Band" })).unwrap();
    assert_eq!(order(&session), ["Lead", "Bass"]);
    assert!(session.project().tracks.iter().all(|t| t.parent.is_none()));
    assert!(session.project().channels.iter().all(|c| c.output.is_none()));
    assert!(run(&mut session, "track.ungroup", json!({ "track": "Bass" })).is_err());

    // The second move changed nothing, so three steps undo it all.
    for _ in 0..3 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn tracks_rename_colour_and_delete() {
    let mut session = Session::new(project());
    run(&mut session, "track.rename", json!({ "track": "Lead", "name": "Keys" })).unwrap();
    run(&mut session, "track.setColor", json!({ "track": "Keys", "color": "#D08F6E" })).unwrap();
    assert_eq!(session.project().tracks[1].color, Some(Rgb(0xD0, 0x8F, 0x6E)));
    assert_eq!(session.project().channels[1].name, "Keys");
    assert!(run(&mut session, "track.rename", json!({ "track": "Keys", "name": "  " })).is_err());
    assert!(run(&mut session, "track.setColor", json!({ "track": "Keys", "color": "red" })).is_err());

    run(&mut session, "track.delete", json!({ "track": "Bass" })).unwrap();
    assert_eq!(order(&session), ["Keys"]);
    assert!(session.project().arrangement.lanes.iter().all(|l| l.track != Some(TrackId(1))));
    assert_eq!(session.project().validate(), Ok(()));
}

#[test]
fn clips_delete_and_duplicate() {
    let mut session = Session::new(project());
    let copy = run(&mut session, "clip.duplicate", json!({ "track": "Bass", "index": 0 })).unwrap();
    assert_eq!(copy["start"], 4.0);
    let starts: Vec<f64> = session.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    assert_eq!(starts, [0.0, 4.0, 8.0]);
    run(&mut session, "clip.delete", json!({ "track": "Bass", "index": 1 })).unwrap();
    let starts: Vec<f64> = session.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    assert_eq!(starts, [0.0, 8.0]);
    assert!(run(&mut session, "clip.delete", json!({ "track": "Bass", "index": 5 })).is_err());
}

/// The test project with its second Bass clip playing an audio content.
fn audio_project() -> Project {
    let mut p = project();
    p.contents.push(ClipContent {
        id: ContentId(9),
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane {
            target: None,
            data: LaneData::Audio(AudioRegion {
                file: "loop.wav".into(),
                absolute_file: None,
                sample_rate: None,
                channels: None,
                duration_seconds: None,
                warped: false,
                stretch: StretchMode::default(),
                warp_markers: Vec::new(),
                gain: 1.0,
                transpose: 0.0,
                stream: None,
            }),
        }],
        extensions: Extensions::default(),
    });
    p.arrangement.lanes[0].clips[1].content = ContentId(9);
    p
}

#[test]
fn an_audio_clip_takes_a_gain_and_a_midi_clip_does_not() {
    let mut session = Session::new(audio_project());
    run(&mut session, "clip.setGain", json!({ "track": "Bass", "index": 1, "db": -6 })).unwrap();
    let LaneData::Audio(region) = &session.project().content(ContentId(9)).unwrap().lanes[0].data else { panic!() };
    assert!((region.gain - 0.501).abs() < 1e-3);
    assert!(run(&mut session, "clip.setGain", json!({ "track": "Bass", "index": 0, "db": 0 })).is_err());
    assert!(run(&mut session, "clip.setGain", json!({ "track": "Bass", "index": 1, "db": 30 })).is_err());
}

#[test]
fn an_audio_clip_takes_fades_that_undo() {
    let mut session = Session::new(audio_project());
    let set = |session: &mut Session, params: Value| run(session, "clip.setFades", params);
    let fades = |session: &Session| {
        let clip = &session.project().arrangement.lanes[0].clips[1];
        (clip.fade_in, clip.fade_out)
    };
    let done = set(&mut session, json!({ "track": "Bass", "index": 1, "in": 1.5 })).unwrap();
    assert_eq!(
        done,
        json!({ "fade_in": 1.5, "fade_out": 0.0, "fade_in_shape": "linear", "fade_in_curve": 0.0,
                "fade_out_shape": "linear", "fade_out_curve": 0.0 })
    );
    set(&mut session, json!({ "track": "Bass", "index": 1, "out": 0.25 })).unwrap();
    assert_eq!(fades(&session), (1.5, 0.25), "out alone keeps the fade-in");
    set(&mut session, json!({ "track": "Bass", "index": 1, "in": 2, "out": 2 })).unwrap();
    assert_eq!(fades(&session), (2.0, 2.0), "together exactly the clip's 4 beats");
    let listed = run(&mut session, "project.clips", json!({ "track": "Bass" })).unwrap();
    assert_eq!((listed[1]["fade_in"].clone(), listed[1]["fade_out"].clone()), (json!(2.0), json!(2.0)));

    // Each is one undo step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(fades(&session), (1.5, 0.25));
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &audio_project());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(fades(&session), (1.5, 0.0));
}

/// Shapes and curves are set alone or with the lengths, listed by project.clips, and undone
/// like any edit (#197).
#[test]
fn fade_shapes_and_curves_are_set_and_undone() {
    let mut session = Session::new(audio_project());
    let set = |session: &mut Session, params: Value| run(session, "clip.setFades", params);
    let curves = |session: &Session| {
        let clip = &session.project().arrangement.lanes[0].clips[1];
        (clip.fade_in_curve, clip.fade_out_curve)
    };
    set(&mut session, json!({ "track": "Bass", "index": 1, "in": 1, "inShape": "equal_power" })).unwrap();
    set(&mut session, json!({ "track": "Bass", "index": 1, "outShape": "s_curve", "outCurve": -0.5 })).unwrap();
    set(&mut session, json!({ "track": "Bass", "index": 1, "inCurve": 1 })).unwrap();
    assert_eq!(
        curves(&session),
        (FadeCurve { shape: FadeShape::EqualPower, curve: 1.0 }, FadeCurve { shape: FadeShape::SCurve, curve: -0.5 })
    );
    let listed = run(&mut session, "project.clips", json!({ "track": "Bass" })).unwrap();
    assert_eq!(
        [
            &listed[1]["fade_in_shape"],
            &listed[1]["fade_in_curve"],
            &listed[1]["fade_out_shape"],
            &listed[1]["fade_out_curve"]
        ],
        [&json!("equal_power"), &json!(1.0), &json!("s_curve"), &json!(-0.5)]
    );
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(curves(&session).0.curve, 0.0, "one undo step per call");
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &audio_project());
}

#[test]
fn bad_fades_are_errors_that_change_nothing() {
    let mut session = Session::new(audio_project());
    let before = session.project().clone();
    for params in [
        json!({ "track": "Bass", "index": 1 }),
        json!({ "track": "Bass", "index": 1, "in": -0.5 }),
        json!({ "track": "Bass", "index": 1, "out": "long" }),
        json!({ "track": "Bass", "index": 1, "in": 3, "out": 1.5 }),
        json!({ "track": "Bass", "index": 1, "in": 5 }),
        json!({ "track": "Bass", "index": 7, "in": 1 }),
        json!({ "track": "Bass", "index": 0, "in": 1 }),
        json!({ "track": "Bass", "index": 1, "inShape": "wobbly" }),
        json!({ "track": "Bass", "index": 1, "outShape": 2 }),
        json!({ "track": "Bass", "index": 1, "inCurve": 1.5 }),
        json!({ "track": "Bass", "index": 1, "outCurve": -2 }),
        json!({ "track": "Bass", "index": 0, "inShape": "fast" }),
    ] {
        assert!(run(&mut session, "clip.setFades", params.clone()).is_err(), "{params}");
    }
    assert_eq!(session.project(), &before);
    assert!(run(&mut session, "edit.undo", json!({})).is_err(), "nothing was recorded");
}

#[test]
fn clips_shortened_or_split_by_a_move_keep_fades_that_fit() {
    let mut p = lane_project();
    for clip in &mut p.arrangement.lanes[0].clips {
        (clip.fade_in, clip.fade_out) = (1.0, 1.0);
    }
    p.arrangement.lanes[0].clips[3].fade_in = 3.0;
    let mut session = Session::new(p);
    // The clip at 4 moves to 9 and covers 9..13: the clip at 8 is shortened to one beat, so its
    // 1 + 1 beats of fades become 0.5 + 0.5.
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 1, "start": 9 })).unwrap();
    let clips = &session.project().arrangement.lanes[0].clips;
    let fades: Vec<(f64, f64, f64, f64)> = clips.iter().map(|c| (c.start, c.length, c.fade_in, c.fade_out)).collect();
    assert!(fades.contains(&(8.0, 1.0, 0.5, 0.5)), "{fades:?}");
    assert_eq!(session.project().validate(), Ok(()));

    // Into the middle of the looping clip (12..20): the first part keeps the fade-in, the
    // second the fade-out.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 14 })).unwrap();
    let clips = &session.project().arrangement.lanes[0].clips;
    let fades: Vec<(f64, f64, f64, f64)> = clips.iter().map(|c| (c.start, c.length, c.fade_in, c.fade_out)).collect();
    assert!(fades.contains(&(12.0, 2.0, 2.0, 0.0)), "{fades:?}");
    assert!(fades.contains(&(18.0, 2.0, 0.0, 1.0)), "{fades:?}");
    assert_eq!(session.project().validate(), Ok(()));
}

#[test]
fn clips_copy_cut_and_paste_with_their_own_content() {
    let mut session = Session::new(project());
    assert!(run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 0 })).is_err(), "nothing copied yet");
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 0 })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Lead", "start": 16 })).unwrap();
    let p = session.project();
    let lead = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    let copy = &lead.clips[usize::try_from(pasted["index"].as_u64().unwrap()).unwrap()];
    assert_eq!(copy.start, 16.0);
    assert_ne!(copy.content, ContentId(1), "the copy has its own content");
    assert_eq!(p.validate(), Ok(()));

    run(&mut session, "clip.cut", json!({ "track": "Bass", "index": 1 })).unwrap();
    assert_eq!(session.project().arrangement.lanes[0].clips.len(), 1);
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 20 })).unwrap();
    assert_eq!(session.project().arrangement.lanes[0].clips.len(), 2);
}

/// The audio test project with an audio track Drums (3), and clip slots in two scenes for
/// Bass and Drums; Lead has none. Drums holds a clip in scene 1; the audio clip on the
/// arrangement has fades, a loop, a gain, warping and an offset.
fn launcher_project() -> Project {
    let mut p = audio_project();
    p.tracks.push(Track {
        id: TrackId(3),
        name: "Drums".into(),
        color: None,
        content: ContentKinds { notes: false, audio: true },
        channel: Some(ChannelId(3)),
        parent: None,
        extensions: Extensions::default(),
    });
    let mut channel = p.channels[0].clone();
    (channel.id, channel.name) = (ChannelId(3), "C3".into());
    p.channels.push(channel);
    if let LaneData::Audio(region) = &mut p.contents[1].lanes[0].data {
        region.gain = 0.5;
        region.warped = true;
        region.stretch = StretchMode::Tonal;
    }
    let audio = &mut p.arrangement.lanes[0].clips[1];
    (audio.fade_in, audio.fade_out, audio.offset) = (1.0, 0.5, 0.25);
    audio.fade_out_curve = FadeCurve { shape: FadeShape::EqualPower, curve: 0.5 };
    audio.loop_range = Some(LoopRange { start: 0.0, end: 2.0 });
    let mut held = p.arrangement.lanes[0].clips[1].clone();
    held.start = 0.0;
    p.launcher = Launcher {
        scenes: vec![Scene::default(), Scene::default()],
        columns: vec![
            LauncherColumn { track: TrackId(1), slots: vec![Slot::default(), Slot::default()] },
            LauncherColumn {
                track: TrackId(3),
                slots: vec![Slot { clip: Some(held), has_stop: true }, Slot::default()],
            },
        ],
    };
    p
}

#[test]
fn an_arrangement_clip_pastes_into_a_slot_with_everything_it_carries() {
    let mut session = Session::new(launcher_project());
    let arranged = session.project().arrangement.lanes[0].clips[1].clone();
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 1 })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Drums", "scene": 2 })).unwrap();
    assert_eq!(pasted, json!({ "track": 3, "scene": 2, "replaced": false }));
    let p = session.project();
    let placed = p.launcher.columns[1].slots[1].clip.clone().unwrap();
    assert_ne!(placed.content, arranged.content, "the slot clip plays its own content");
    assert_eq!(Clip { content: arranged.content, start: arranged.start, ..placed.clone() }, arranged);
    assert_eq!(placed.start, 0.0);
    let (copy, original) = (p.content(placed.content).unwrap(), p.content(arranged.content).unwrap());
    assert_eq!(copy.lanes, original.lanes, "gain and warping travel with the content");
    assert_eq!(p.arrangement.lanes[0].clips.len(), 2, "the arrangement is left as it was");
    assert_eq!(p.validate(), Ok(()));

    // One undo step takes it out again, content and all.
    let contents = p.contents.len();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().launcher.columns[1].slots[1].clip.is_none());
    assert_eq!(session.project().contents.len(), contents - 1);
}

#[test]
fn a_paste_into_a_held_slot_replaces_its_clip() {
    let mut session = Session::new(launcher_project());
    let before = session.project().launcher.columns[1].slots[0].clone();
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 1 })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": 3, "scene": 1 })).unwrap();
    assert_eq!(pasted["replaced"], true);
    let after = &session.project().launcher.columns[1].slots[0];
    assert_ne!(after.clip.as_ref().map(|c| c.content), before.clip.as_ref().map(|c| c.content));
    assert!(after.has_stop, "the slot keeps its stop button");
}

#[test]
fn a_slot_clip_copies_onto_the_arrangement() {
    let mut session = Session::new(launcher_project());
    run(&mut session, "clip.copy", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Drums", "start": 16 })).unwrap();
    assert_eq!((pasted["track"].as_u64(), pasted["start"].as_f64()), (Some(3), Some(16.0)));
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    let slot = p.launcher.columns[1].slots[0].clip.clone().unwrap();
    assert_eq!(lane.clips.len(), 1);
    assert_eq!(Clip { content: slot.content, start: 0.0, ..lane.clips[0].clone() }, slot);
    assert_eq!(p.validate(), Ok(()));
    // A MIDI clip from a slot goes to a MIDI track's slot too.
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 0 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "scene": 1 })).unwrap();
    run(&mut session, "clip.copy", json!({ "track": "Bass", "scene": 1 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "scene": 2 })).unwrap();
    assert!(session.project().launcher.columns[0].slots.iter().all(|s| s.clip.is_some()));
}

#[test]
fn malformed_copies_and_pastes_into_slots_are_errors_that_record_nothing() {
    let mut session = Session::new(launcher_project());
    let copy = |session: &mut Session, params: Value| run(session, "clip.copy", params);
    for params in [
        json!({ "track": "Bass" }),
        json!({ "track": "Bass", "index": 0, "scene": 1 }),
        json!({ "track": "Bass", "scene": 1 }),
        json!({ "track": "Drums", "scene": 3 }),
        json!({ "track": "Drums", "scene": 0 }),
        json!({ "track": "Drums", "scene": -1 }),
        json!({ "track": "Drums", "scene": "one" }),
        json!({ "track": "Lead", "scene": 1 }),
    ] {
        assert!(copy(&mut session, params.clone()).is_err(), "{params}");
    }
    assert!(run(&mut session, "clip.paste", json!({ "track": "Drums", "scene": 2 })).is_err(), "nothing copied");
    assert!(
        run(&mut session, "clip.cut", json!({ "track": "Drums", "scene": 1 })).is_err(),
        "cut is for the arrangement"
    );

    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 1 })).unwrap();
    let before = session.project().clone();
    for params in [
        json!({ "track": "Drums" }),
        json!({ "track": "Drums", "start": 4, "scene": 2 }),
        json!({ "track": "Drums", "scene": 2, "stack": true }),
        json!({ "track": "Drums", "scene": 3 }),
        json!({ "track": "Drums", "scene": 0 }),
        json!({ "track": "Drums", "scene": 1.5 }),
        json!({ "track": "Drums", "scene": null }),
        json!({ "track": "Lead", "scene": 1 }),
        json!({ "track": "Nobody", "scene": 1 }),
        json!({ "track": "Drums", "start": -1 }),
        json!({ "track": "Drums", "start": 1e12 }),
    ] {
        assert!(run(&mut session, "clip.paste", params.clone()).is_err(), "{params}");
    }
    // Audio onto a MIDI track's slot, and MIDI onto an audio track's.
    assert!(run(&mut session, "clip.paste", json!({ "track": "Bass", "scene": 2 })).is_err());
    run(&mut session, "clip.copy", json!({ "track": "Bass", "index": 0 })).unwrap();
    assert!(run(&mut session, "clip.paste", json!({ "track": "Drums", "scene": 2 })).is_err());
    assert_eq!(session.project(), &before);
    assert!(run(&mut session, "edit.undo", json!({})).is_err(), "nothing was recorded");
}
