use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

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
fn continuous_parameter_updates_share_one_undo_step() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    for value in [1.0, 2.0, 3.0] {
        run(
            &mut session,
            "device.setParam",
            json!({ "track": 1, "device": 0, "param": "gain", "value": value, "continuous": true }),
        )
        .unwrap();
    }
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "gain", "value": 3.0 })).unwrap();

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels[0].devices[0].params.is_empty());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels[0].devices.is_empty());
}

#[test]
fn saving_during_a_parameter_gesture_keeps_later_updates_dirty() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let path = std::env::temp_dir().join(format!("suhina-param-gesture-{}.suhina", std::process::id()));
    for value in [1.0, 2.0, 3.0] {
        run(
            &mut session,
            "device.setParam",
            json!({ "track": 1, "device": 0, "param": "gain", "value": value, "continuous": true }),
        )
        .unwrap();
        assert!(session.has_unsaved_changes());
        if value < 3.0 {
            run(&mut session, "project.save", json!({ "path": path })).unwrap();
            assert!(!session.has_unsaved_changes());
        }
    }
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "gain", "value": 3.0 })).unwrap();
    assert!(session.has_unsaved_changes());
    let (saved, _) = Session::open(&path).unwrap();
    assert_eq!(saved.project().channels[0].devices[0].params[0].value, 2.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels[0].devices[0].params.is_empty());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].devices[0].params[0].value, 3.0);
    std::fs::remove_file(path).unwrap();
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

#[test]
fn sd_effects_load_into_tracks_and_expose_their_parameters() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("org.example.sd-gain", "SDK Gain", suhina_device_sdk::DeviceRole::Effect);
    graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Output", (0.0, 2.0), 1.0));
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("input"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("output"),
    ];
    graph.nodes[1].visible = false;
    graph.connections =
        vec![suhina_device_sdk::Connection::new("input", "gain"), suhina_device_sdk::Connection::new("gain", "output")];
    let path = std::env::temp_dir().join(format!("suhina-test-{}-gain.sd", std::process::id()));
    std::fs::write(&path, graph.to_sd().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    let mut session = Session::new(project());
    let added = run(&mut session, "device.addSd", json!({ "track": 1, "path": path })).unwrap();
    assert_eq!(added["name"], "SDK Gain");
    assert_eq!(session.project().channels[0].devices[0].kind, DeviceKind::Sd);
    let listed = run(&mut session, "track.devices", json!({ "track": 1 })).unwrap();
    assert_eq!(listed[0]["params"][0]["name"], "Output");
    assert_eq!(listed[0]["nodes"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        run(
            &mut session,
            "device.setParam",
            json!({
                "track": 1, "device": 0, "param": "gain", "value": 0.25
            })
        )
        .unwrap()["value"],
        0.25
    );
    assert_eq!(session.project().channels[0].devices[0].params[0].value, 0.25);
    let created = run(&mut session, "track.add", json!({ "sd": path.display().to_string() })).unwrap();
    assert_eq!(created["kind"], "audio");
    assert_eq!(created["device"]["kind"], "sd");
    let _ = std::fs::remove_file(path);
}

/// A catalog with one installed effect and one instrument, as a scan would have found them.
fn plugin_env() -> std::sync::Arc<suhina_session::PluginEnv> {
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let mut catalog = Catalog::new();
    let plugin = |file: &str, id: &str, name: &str, features: &[&str]| PluginInfo {
        path: std::path::PathBuf::from(file),
        id: id.into(),
        name: name.into(),
        vendor: "Vendor".into(),
        version: "1.0".into(),
        features: features.iter().map(|f| f.to_string()).collect(),
        replaces: Vec::new(),
    };
    let verb = plugin("Verb.vst3", "ABCDEF0123456789ABCDEF0123456789", "Verb", &["Fx|Reverb"]);
    let synth = plugin("Synth.clap", "com.vendor.synth", "Synth", &["instrument", "synthesizer"]);
    catalog.insert(verb.path.clone(), vec![verb]);
    catalog.insert(synth.path.clone(), vec![synth]);
    std::sync::Arc::new(suhina_session::PluginEnv::new(catalog, "no-such-host".into()))
}

#[test]
fn installed_plug_ins_are_added_from_the_catalog() {
    let mut session = Session::new(project());
    // Without a catalog there is nothing to add from.
    let error = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Verb" })).unwrap_err();
    assert!(error.contains("catalog"), "{error}");
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let verb = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "verb" })).unwrap();
    assert_eq!(
        (verb["device"].clone(), verb["format"].clone(), verb["instrument"].clone()),
        (json!(1), json!("vst3"), json!(false))
    );
    // An instrument goes first, and only first.
    let synth = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "com.vendor.synth" })).unwrap();
    assert_eq!(synth["device"], 0);
    assert!(run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Synth", "position": 2 })).is_err());
    assert!(run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Nothing" })).is_err());
    let devices = &session.project().channels[0].devices;
    assert_eq!(devices[0].kind, DeviceKind::Clap { id: "com.vendor.synth".into() });
    assert_eq!(devices[2].kind, DeviceKind::Vst3 { id: "ABCDEF0123456789ABCDEF0123456789".into() });
    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].devices.len(), 2);
}

#[test]
fn plug_in_commands_need_a_plug_in_and_a_playing_project() {
    let mut session = Session::new(project());
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Verb" })).unwrap();
    let device = json!({ "track": 1, "device": 1 });
    // Not playing: its plug-in does not run, and there is no editor to open.
    assert_eq!(run(&mut session, "device.plugin", device.clone()).unwrap()["status"], "stopped");
    let listed = run(&mut session, "track.devices", json!({ "track": 1 })).unwrap();
    assert_eq!(listed[1]["plugin"]["status"], "stopped");
    assert!(listed[0].get("plugin").is_none(), "a built-in effect or instrument has no plug-in");
    assert!(run(&mut session, "device.openEditor", device.clone()).is_err());
    assert!(run(&mut session, "device.restart", device).is_err());
    let built_in = run(&mut session, "device.openEditor", json!({ "track": 1, "device": 0 })).unwrap_err();
    assert!(built_in.contains("not a plug-in"), "{built_in}");
    let kinds: Vec<(&str, &str)> = suhina_session::commands()
        .iter()
        .filter(|c| c.id.starts_with("device.") && c.id.contains("Editor") || c.id == "device.restart")
        .map(|c| (c.id, c.kind.name()))
        .collect();
    assert_eq!(
        kinds,
        [("device.openEditor", "plugin"), ("device.closeEditor", "plugin"), ("device.restart", "plugin")]
    );
}

#[test]
fn sdk_catalog_is_a_read_command_and_matches_the_sdk() {
    let mut session = Session::new(project());
    let catalog = run(&mut session, "devices.sdNodes", json!({})).unwrap();
    assert_eq!(catalog["version"], 2);
    assert_eq!(catalog["nodes"].as_array().unwrap().len(), suhina_device_sdk::NODE_CATALOG.len());
    assert!(
        catalog["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == "delay" && n["inputs"][1]["name"] == "time")
    );
    assert!(run(&mut session, "edit.undo", json!({})).is_err());
}

#[test]
fn branching_sd_graph_and_discrete_parameters_survive_project_save_and_source_removal() {
    use suhina_device_sdk::{
        Connection as C, DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter, ParameterControl,
    };
    let mut g = DeviceGraph::new("org.test.stereo", "Stereo", DeviceRole::Effect);
    let mut p = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    p.control = ParameterControl::Integer;
    p.unit = "steps".into();
    g.parameters.push(p);
    g.nodes = vec![N::audio_input("in"), N::new("stereo", K::Stereo), N::gain("gain", "gain"), N::audio_output("out")];
    g.connections = vec![
        C::ports("in", "left", "stereo", "right"),
        C::ports("in", "right", "stereo", "left"),
        C::new("stereo", "gain"),
        C::new("gain", "out"),
    ];
    let path = std::env::temp_dir().join(format!("suhina-sdk-branching-{}.sd", std::process::id()));
    g.write_file(&path).unwrap();
    let mut session = Session::new(project());
    run(&mut session, "device.addSd", json!({"track":1,"path":path})).unwrap();
    std::fs::remove_file(&path).unwrap();
    let listed = run(&mut session, "track.devices", json!({"track":1})).unwrap();
    assert_eq!(listed[0]["params"][0]["control"], "integer");
    assert_eq!(listed[0]["params"][0]["unit"], "steps");
    let changed =
        run(&mut session, "device.setParam", json!({"track":1,"device":0,"param":"gain","value":1.6})).unwrap();
    assert_eq!(changed["value"], 2.0);
    let restored = suhina_project_file::from_json(&suhina_project_file::to_json(session.project())).unwrap();
    let device = &restored.channels[0].devices[0];
    let graph = DeviceGraph::from_sd(std::str::from_utf8(device.state.as_deref().unwrap()).unwrap()).unwrap();
    assert_eq!(graph, g);
    assert_eq!(device.params[0].value, 2.0);
    let mut effect = graph.compile().unwrap();
    effect.set_parameter(0, device.params[0].value);
    let mut out = [1.0, 2.0];
    effect.process(&mut out);
    assert_eq!(out, [4.0, 2.0]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].devices[0].params[0].value, 1.0);
}

#[test]
fn xy_gesture_groups_axes_and_ends_on_unchanged_value() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"track":1,"id":"utility"})).unwrap();
    let before = session.project().clone();
    for (param, value, continuous) in [("gain", 2.0, true), ("balance", 0.5, true), ("balance", 0.5, false)] {
        run(
            &mut session,
            "device.setParam",
            json!({"track":1,"device":0,"param":param,"value":value,"continuous":continuous,"gesture":"xy"}),
        )
        .unwrap();
    }
    let after = session.project().clone();
    run(
        &mut session,
        "device.setParam",
        json!({"track":1,"device":0,"param":"gain","value":3.0,"continuous":false,"gesture":"xy"}),
    )
    .unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &after);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before);
    assert!(
        run(&mut session, "device.setParam", json!({"track":1,"device":0,"param":"gain","value":3.0,"gesture":""}))
            .is_err()
    );
}
