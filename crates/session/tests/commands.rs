use loopwerk_model::*;
use loopwerk_session::{CommandKind, Session, commands};
use serde_json::{Value, json};

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
