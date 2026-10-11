//! Transport and launcher commands drive a real renderer through its controller, rendered
//! offline here instead of on an audio device.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use loopwerk_engine::{AudioPool, Quantization, Renderer};
use loopwerk_model::*;
use loopwerk_session::{CommandKind, Player, Session, commands, parse_quantization};
use serde_json::{Value, json};

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;

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
    let content = |id: u32, name: &str, key: u8| ClipContent {
        id: ContentId(id),
        name: name.into(),
        length: 1.0,
        lanes: vec![ContentLane {
            target: None,
            data: LaneData::Notes(vec![Note {
                time: 0.0,
                duration: 0.25,
                key,
                velocity: 1.0,
                release_velocity: 0.5,
                channel: 0,
            }]),
        }],
        extensions: Extensions::default(),
    };
    let clip = |content: u32, enabled: bool| Clip {
        content: ContentId(content),
        start: 0.0,
        length: 1.0,
        offset: 0.0,
        loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
        enabled,
        extensions: Extensions::default(),
    };
    let slot = |clip: Option<Clip>, has_stop: bool| Slot { clip, has_stop };
    Project {
        tracks: vec![track(1, "Drums"), track(2, "Bass"), track(3, "Pad")],
        channels: vec![channel(1), channel(2), channel(3)],
        contents: vec![content(1, "Beat", 36), content(2, "Line", 40), content(3, "Off", 50)],
        launcher: Launcher {
            scenes: vec![
                Scene { name: "Intro".into(), tempo_bpm: None },
                Scene { name: "Drop".into(), tempo_bpm: Some(128.0) },
            ],
            columns: vec![
                LauncherColumn {
                    track: TrackId(1),
                    slots: vec![slot(Some(clip(1, true)), true), slot(Some(clip(1, true)), true)],
                },
                LauncherColumn { track: TrackId(2), slots: vec![slot(None, true), slot(Some(clip(2, true)), true)] },
                // A deactivated clip, and an empty slot without a stop button.
                LauncherColumn { track: TrackId(3), slots: vec![slot(Some(clip(3, false)), true), slot(None, false)] },
            ],
        },
        ..Project::default()
    }
}

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

/// A session with an offline renderer attached as its player.
fn playing() -> (Session, Renderer) {
    let project = project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    (session, renderer)
}

fn advance(renderer: &mut Renderer, frames: usize) {
    let mut buffer = vec![0.0f32; 2 * 256];
    let mut left = frames;
    while left > 0 {
        let n = left.min(256);
        renderer.process(&mut buffer[..2 * n]);
        left -= n;
    }
}

#[test]
fn transport_commands_need_a_player() {
    let mut session = Session::new(project());
    for spec in commands().iter().filter(|c| c.kind == CommandKind::Transport) {
        let params = json!({ "track": "Drums", "scene": 1, "value": "1 bar", "on": true });
        let params: Value = params
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| spec.params.iter().any(|p| p.name == k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<serde_json::Map<_, _>>()
            .into();
        let err = run(&mut session, spec.id, params).unwrap_err();
        assert!(err.contains("no audio engine"), "{}: {err}", spec.id);
    }
    // The launcher grid is part of the project and needs no player.
    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    assert_eq!(grid["scenes"][1]["name"], "Drop");
    assert_eq!(grid["scenes"][1]["tempo_bpm"], 128.0);
    assert_eq!(grid["tracks"][0]["slots"][0]["clip"], "Beat");
    assert_eq!(grid["tracks"][1]["slots"][0]["stop_button"], true);
}

#[test]
fn launching_a_scene_plays_its_row_and_changes_the_tempo() {
    let (mut session, mut renderer) = playing();
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], false);

    run(&mut session, "scene.launch", json!({ "scene": 2 })).unwrap();
    advance(&mut renderer, 100);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], true);
    assert_eq!(state["tempo_bpm"], 128.0);
    assert_eq!(state["tracks"][0]["playing"]["scene"], 2);
    assert_eq!(state["tracks"][1]["playing"]["clip"], "Line");
    // Scene 2's slot on track 3 is empty without a stop button: nothing changes there.
    assert_eq!(state["tracks"][2]["playing"], Value::Null);
    assert_eq!(state["tracks"][0]["plays_arrangement"], false);
    assert_eq!(state["tracks"][2]["plays_arrangement"], true);
}

#[test]
fn launches_and_stops_are_queued_until_the_next_bar() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT);
    let launched = run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 2 })).unwrap();
    assert_eq!(launched["queued"], "launch");
    run(&mut session, "track.stop", json!({ "track": 1 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][0]["playing"]["scene"], 1);
    assert_eq!(state["tracks"][0]["queued"], "stop");
    assert_eq!(state["tracks"][1]["queued"]["clip"], "Line");
    assert_eq!(state["position"], "1.2");

    advance(&mut renderer, 3 * BEAT);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["position"], "2.1");
    assert_eq!(state["tracks"][0]["playing"], Value::Null);
    assert_eq!(state["tracks"][1]["playing"]["scene"], 2);
    assert_eq!(state["tracks"][1]["queued"], Value::Null);

    // An empty slot with a stop button stops its track.
    let stopped = run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 1 })).unwrap();
    assert_eq!(stopped["queued"], "stop");

    run(&mut session, "transport.stop", json!({})).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], false);
    assert_eq!(state["tracks"][1]["playing"], Value::Null);
    assert_eq!(state["tracks"][1]["queued"], Value::Null);
}

#[test]
fn bad_launches_are_errors_and_change_nothing() {
    let (mut session, mut renderer) = playing();
    let before = session.project().clone();
    let bad = [
        ("clip.launch", json!({ "track": "Drums", "scene": 0 })),
        ("clip.launch", json!({ "track": "Drums", "scene": 3 })),
        ("clip.launch", json!({ "track": "Nope", "scene": 1 })),
        ("clip.launch", json!({ "track": "Drums", "scene": -1 })),
        ("clip.launch", json!({ "track": "Drums", "scene": "1" })),
        // A deactivated clip, and an empty slot without a stop button.
        ("clip.launch", json!({ "track": "Pad", "scene": 1 })),
        ("clip.launch", json!({ "track": "Pad", "scene": 2 })),
        ("scene.launch", json!({ "scene": u64::MAX })),
        ("track.stop", json!({ "track": 99 })),
        ("launcher.setQuantization", json!({ "value": "3 bars" })),
        ("launcher.setQuantization", json!({ "value": 4 })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    advance(&mut renderer, BEAT);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], false);
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn quantisation_values_parse_to_beats() {
    assert_eq!(parse_quantization("none").unwrap(), Quantization::None);
    assert_eq!(parse_quantization(" 2 Bars ").unwrap(), Quantization::Bars(2));
    assert_eq!(parse_quantization("1 bar").unwrap(), Quantization::Bars(1));
    assert_eq!(parse_quantization("1/4").unwrap(), Quantization::Beats(1.0));
    assert_eq!(parse_quantization("1/16").unwrap(), Quantization::Beats(0.25));
    assert!(matches!(parse_quantization("1/8T").unwrap(), Quantization::Beats(b) if (b - 1.0 / 3.0).abs() < 1e-12));
    for bad in ["", "1/3", "1/32T", "bar", "16 bars"] {
        assert!(parse_quantization(bad).is_err(), "{bad}");
    }
}

#[test]
fn quantisation_none_launches_at_once() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT / 3);
    run(&mut session, "launcher.setQuantization", json!({ "value": "none" })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 2 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][1]["playing"]["scene"], 2);
}

/// Renders `frames` and returns the loudest sample of each beat.
fn beat_peaks(renderer: &mut Renderer, beats: usize) -> Vec<f32> {
    let mut buffer = vec![0.0f32; 2 * BEAT];
    (0..beats)
        .map(|_| {
            renderer.process(&mut buffer);
            buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()))
        })
        .collect()
}

#[test]
fn edits_and_undo_are_heard_while_playing() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let loud = beat_peaks(&mut renderer, 2)[1];
    assert!(loud > 0.05);

    // -12 dB is a quarter of the level, from the next note on.
    run(&mut session, "track.setVolume", json!({ "track": "Drums", "db": -12 })).unwrap();
    let quieter = beat_peaks(&mut renderer, 2)[1];
    assert!((quieter / loud - 0.25).abs() < 0.01, "{loud} -> {quieter}");
    // The clip kept playing: the launcher still reports it.
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], true);

    run(&mut session, "edit.undo", json!({})).unwrap();
    let back = beat_peaks(&mut renderer, 2)[1];
    assert!((back / loud - 1.0).abs() < 0.01, "{loud} -> {back}");
}

#[test]
fn the_metronome_is_a_transport_command() {
    let (mut session, mut renderer) = playing();
    let set = run(&mut session, "transport.setMetronome", json!({ "on": true, "db": -12 })).unwrap();
    assert_eq!(set, json!({ "metronome": true, "db": -12.0 }));
    run(&mut session, "transport.play", json!({})).unwrap();
    let peaks = beat_peaks(&mut renderer, 2);
    // A click at -12 dB on each beat, and no undo step.
    assert!(peaks.iter().all(|p| (0.2..0.26).contains(p)), "{peaks:?}");
    assert_eq!(run(&mut session, "launcher.state", json!({})).unwrap()["metronome"], true);
    assert!(!session.can_undo());
    assert!(run(&mut session, "transport.setMetronome", json!({ "on": true, "db": 7 })).is_err());
}
