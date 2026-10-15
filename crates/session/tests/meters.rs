//! The level meters' commands read a real renderer's meters through its controller, rendered
//! offline here instead of on an audio device.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

const SR: u32 = 48_000;

/// Two tracks, "Lead" and "Quiet" (at -inf, so never heard), each playing an A4 for 16 beats.
fn project(volume: f64) -> Project {
    let track = |id: u32, name: &str| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        parent: None,
        extensions: Extensions::default(),
    };
    let channel = |id: u32, volume: f64| Channel {
        id: ChannelId(id),
        name: format!("C{id}"),
        role: ChannelRole::Regular,
        volume,
        pan: 0.0,
        mute: false,
        solo: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    };
    let note =
        Note { time: 0.0, duration: 16.0, key: 69, velocity: 1.0, release_velocity: 0.5, channel: 0, muted: false };
    let clip = Clip {
        content: ContentId(1),
        start: 0.0,
        length: 16.0,
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
    Project {
        tracks: vec![track(1, "Lead"), track(2, "Quiet")],
        channels: vec![channel(1, volume), channel(2, 0.0)],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "A".into(),
            length: 16.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note]) }],
            extensions: Extensions::default(),
        }],
        arrangement: Arrangement {
            lanes: [1, 2]
                .iter()
                .map(|&t| ArrangementLane { name: format!("l{t}"), track: Some(TrackId(t)), clips: vec![clip.clone()] })
                .collect(),
        },
        ..Project::default()
    }
}

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

/// A session at `volume` on "Lead", playing for two seconds in an offline renderer.
fn played(volume: f64) -> (Session, Renderer) {
    let project = project(volume);
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    run(&mut session, "transport.play", json!({})).unwrap();
    let mut block = vec![0.0f32; 2 * 256];
    for _ in 0..(2 * SR as usize / 256) {
        renderer.process(&mut block);
    }
    (session, renderer)
}

#[test]
fn levels_reads_the_main_output_and_every_track_in_db() {
    let (mut session, _renderer) = played(1.0);
    let levels = run(&mut session, "mixer.levels", json!({})).unwrap();
    // One centred note at unity: a quarter of full scale, -12 dB, its RMS 3 dB lower.
    assert_eq!(levels["main"]["peak_db"], json!([-12.0, -12.0]));
    assert_eq!(levels["main"]["rms_db"], json!([-15.1, -15.1]));
    assert_eq!(levels["main"]["clipped"], json!(false));
    let tracks = levels["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0]["track"], json!(1));
    assert_eq!(tracks[0]["track_name"], json!("Lead"));
    assert_eq!(tracks[0]["peak_db"], json!([-12.0, -12.0]));
    // Silence reads null.
    assert_eq!(tracks[1]["track_name"], json!("Quiet"));
    assert_eq!(tracks[1]["peak_db"], json!([null, null]));
    assert_eq!(tracks[1]["rms_db"], json!([null, null]));
}

#[test]
fn reset_clip_turns_off_a_track_the_main_output_or_every_indicator() {
    // +18 dB: twice full scale on the track and at the output.
    let (mut session, _renderer) = played(8.0);
    let clipped = |session: &mut Session| {
        let levels = run(session, "mixer.levels", json!({})).unwrap();
        (levels["tracks"][0]["clipped"].as_bool().unwrap(), levels["main"]["clipped"].as_bool().unwrap())
    };
    assert_eq!(clipped(&mut session), (true, true));
    run(&mut session, "mixer.resetClip", json!({ "track": "Lead" })).unwrap();
    assert_eq!(clipped(&mut session), (false, true));
    run(&mut session, "mixer.resetClip", json!({ "main": true })).unwrap();
    assert_eq!(clipped(&mut session), (false, false));

    let (mut session, _renderer) = played(8.0);
    run(&mut session, "mixer.resetClip", json!({})).unwrap();
    assert_eq!(clipped(&mut session), (false, false));
    assert!(run(&mut session, "mixer.resetClip", json!({ "track": "Nobody" })).is_err());
    assert!(run(&mut session, "mixer.resetClip", json!({ "main": "yes" })).is_err());
    // Neither command records an undo step.
    assert!(!session.can_undo());
}

#[test]
fn the_meters_need_a_playing_engine() {
    let mut session = Session::new(project(1.0));
    let error = run(&mut session, "mixer.levels", json!({})).unwrap_err();
    assert!(error.contains("no audio engine"), "{error}");
    assert!(run(&mut session, "mixer.resetClip", json!({})).is_err());
}
