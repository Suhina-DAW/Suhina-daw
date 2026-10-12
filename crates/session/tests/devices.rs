use loopwerk_model::*;
use loopwerk_session::Session;
use serde_json::{Value, json};

fn project() -> Project {
    Project {
        tracks: vec![Track {
            id: TrackId(1),
            name: "Keys".into(),
            color: None,
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            parent: None,
            extensions: Extensions::default(),
        }],
        channels: vec![Channel {
            id: ChannelId(1),
            name: "C1".into(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: Vec::new(),
            devices: Vec::new(),
            extensions: Extensions::default(),
        }],
        ..Project::default()
    }
}

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

#[test]
fn lists_the_built_in_devices_and_their_parameters() {
    let mut session = Session::new(project());
    let list = run(&mut session, "devices.list", json!({})).unwrap();
    let fm = list.as_array().unwrap().iter().find(|d| d["id"] == "fm-synth").unwrap();
    assert_eq!(fm["role"], "instrument");
    let algorithm = fm["params"].as_array().unwrap().iter().find(|p| p["id"] == "algorithm").unwrap();
    assert_eq!(
        (algorithm["kind"].as_str(), algorithm["min"].as_f64(), algorithm["max"].as_f64()),
        (Some("integer"), Some(1.0), Some(32.0))
    );
    let utility = list.as_array().unwrap().iter().find(|d| d["id"] == "utility").unwrap();
    assert_eq!(utility["role"], "effect");
    let gain = utility["params"].as_array().unwrap().iter().find(|p| p["id"] == "gain").unwrap();
    assert_eq!(gain["unit"], "dB");
}

#[test]
fn devices_are_added_set_switched_and_removed_with_undo() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": "Keys", "id": "utility" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "Keys", "id": "overdrive" })).unwrap();
    // An instrument always goes first.
    let added = run(&mut session, "device.add", json!({ "track": "Keys", "id": "fm-synth" })).unwrap();
    assert_eq!(added["device"], 0);
    let set =
        run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "algorithm", "value": 40 }))
            .unwrap();
    assert_eq!(set["value"], 32.0, "limited to the range");
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "algorithm", "value": 5.4 }))
        .unwrap();
    run(&mut session, "device.setEnabled", json!({ "track": 1, "device": 2, "enabled": false })).unwrap();

    let devices = run(&mut session, "track.devices", json!({ "track": "Keys" })).unwrap();
    let names: Vec<&str> = devices.as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["FM Synth", "Utility", "Overdrive"]);
    let algorithm = devices[0]["params"].as_array().unwrap().iter().find(|p| p["id"] == "algorithm").unwrap();
    assert_eq!(algorithm["value"], 5.0);
    assert_eq!(devices[2]["enabled"], false);
    assert_eq!(session.project().channels[0].devices[0].params, [DeviceParam::new("algorithm", 5.0)]);

    run(&mut session, "device.remove", json!({ "track": 1, "device": 1 })).unwrap();
    assert_eq!(session.project().channels[0].devices.len(), 2);
    for _ in 0..7 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn bad_device_edits_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth" })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("device.add", json!({ "track": 1, "id": "no-such-device" })),
        ("device.add", json!({ "track": 1, "id": "fm-synth" })),
        ("device.add", json!({ "track": 1, "id": "utility", "position": 0 })),
        ("device.add", json!({ "track": 1, "id": "utility", "position": 5 })),
        ("device.add", json!({ "track": 9, "id": "utility" })),
        ("device.remove", json!({ "track": 1, "device": 3 })),
        ("device.setParam", json!({ "track": 1, "device": 0, "param": "nothing", "value": 1 })),
        ("device.setParam", json!({ "track": 1, "device": 0, "param": "feedback", "value": "a lot" })),
        ("device.setParam", json!({ "track": 1, "device": 1, "param": "feedback", "value": 1 })),
        ("device.setEnabled", json!({ "track": 1, "device": 0, "enabled": "yes" })),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
        assert_eq!(session.project(), &before);
    }
    // One undo step for the one good edit.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(run(&mut session, "edit.undo", json!({})).is_err());
}
