//! Plug-ins in the engine's track chains (ADR 0006, issue #100), with the plug-ins built into
//! the host: a project's plug-in effect and instrument in an offline render, a plug-in that
//! crashes or hangs while the engine plays on, and a session that finds, starts, watches,
//! restarts and opens the editor of its project's plug-ins through commands.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use loopwerk_engine::{AudioPool, Renderer, plugin_devices, render_offline, render_offline_with_plugins};
use loopwerk_model::*;
use loopwerk_plugins::scan::{Catalog, PluginInfo};
use loopwerk_plugins::{Health, Launch, Plugin};
use loopwerk_session::{Hosted, Player, PluginEnv, Session};
use serde_json::json;

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;
const PATIENT: Duration = Duration::from_secs(20);

fn host() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_loopwerk-plugin-host"))
}

/// A catalog that holds the host's test plug-ins under their own names, each with the id
/// `<name>` (`test:gain` has the id `gain`), instruments marked as such.
fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for (name, features) in [
        ("gain", vec!["audio-effect"]),
        ("vst3", vec!["Instrument|Synth"]),
        ("clap", vec!["audio-effect"]),
        ("crash-after:40", vec!["audio-effect"]),
        ("hang-after:40", vec!["audio-effect"]),
    ] {
        let path = PathBuf::from(format!("test:{name}"));
        let info = PluginInfo {
            path: path.clone(),
            id: name.into(),
            name: format!("Test {name}"),
            vendor: "Loopwerk".into(),
            version: "1".into(),
            features: features.into_iter().map(String::from).collect(),
            replaces: Vec::new(),
        };
        catalog.insert(path, vec![info]);
    }
    catalog
}

fn env() -> Arc<PluginEnv> {
    Arc::new(PluginEnv::new(catalog(), host()))
}

fn plugin_device(id: &str) -> Device {
    Device {
        name: format!("Test {id}"),
        kind: DeviceKind::Clap { id: id.into() },
        enabled: true,
        state: None,
        params: Vec::new(),
        extensions: Extensions::default(),
    }
}

/// One track playing a note from beat 0 to `length` beats (and one at beat 1 when `two`),
/// with `devices`.
fn project(length: f64, two: bool, devices: Vec<Device>) -> Project {
    let note =
        |time: f64, duration: f64| Note { time, duration, key: 69, velocity: 1.0, release_velocity: 0.5, channel: 0 };
    let notes = if two { vec![note(0.0, 0.5), note(1.0, 0.5)] } else { vec![note(0.0, length)] };
    Project {
        tracks: vec![Track {
            id: TrackId(1),
            name: "T".into(),
            color: None,
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            parent: None,
            extensions: Extensions::default(),
        }],
        channels: vec![Channel {
            id: ChannelId(1),
            name: "C".into(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: Vec::new(),
            devices,
            extensions: Extensions::default(),
        }],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
            extensions: Extensions::default(),
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip {
                    content: ContentId(1),
                    start: 0.0,
                    length,
                    offset: 0.0,
                    loop_range: None,
                    enabled: true,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    extensions: Extensions::default(),
                }],
            }],
        },
        ..Project::default()
    }
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

#[test]
fn an_offline_render_runs_a_projects_plug_in_effect() {
    let p = project(2.0, false, vec![plugin_device("gain")]);
    let started = loopwerk_session::start_offline(&p, &env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
    let wet = render_offline_with_plugins(&p, &AudioPool::new(), SR, 512, started.plugins).unwrap();
    // Without the plug-in (not installed) the sound passes; the test gain halves it. The chain
    // adds a tail for effects to ring out.
    let dry = render_offline(&p, SR, 512).unwrap();
    assert!(wet.len() > dry.len());
    let half: Vec<f32> = dry.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&wet[..dry.len()], &half) < 1e-6, "{}", max_diff(&wet[..dry.len()], &half));
    assert!(wet[dry.len()..].iter().all(|s| s.abs() < 1e-6));
    assert!(dry.iter().any(|s| s.abs() > 0.1));
}

#[test]
fn an_offline_render_plays_notes_on_a_plug_in_instrument() {
    // The VST3 test plug-in answers each note with its velocity on the note's frame and
    // reports 32 frames of latency; it is the track's instrument, so no sine voices play.
    let mut device = plugin_device("vst3");
    device.state = Some(1.0f32.to_le_bytes().to_vec());
    let p = project(2.0, true, vec![device]);
    let started = loopwerk_session::start_offline(&p, &env(), SR);
    assert!(started.notices.is_empty(), "{:?}", started.notices);
    let out = render_offline_with_plugins(&p, &AudioPool::new(), SR, 256, started.plugins).unwrap();
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
    let loud: Vec<usize> = left.iter().enumerate().filter(|(_, s)| s.abs() > 1e-6).map(|(i, _)| i).collect();
    assert_eq!(loud, [0, BEAT], "notes land on their frames");
    assert!((left[0] - 1.0).abs() < 1e-6, "{}", left[0]);
}

/// Runs `blocks` blocks of 480 frames as playback does (with a real-time deadline) and returns
/// the output and the longest a block took.
fn play(renderer: &mut Renderer, blocks: usize) -> (Vec<f32>, Duration) {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; 480 * 2];
    let mut longest = Duration::ZERO;
    for _ in 0..blocks {
        let started = Instant::now();
        renderer.process(&mut block);
        longest = longest.max(started.elapsed());
        out.extend_from_slice(&block);
    }
    (out, longest)
}

fn start(spec: &str) -> (Plugin, Hosted) {
    let launch = Launch {
        host: host(),
        plugin: spec.into(),
        channels: 2,
        max_frames: loopwerk_engine::MAX_BLOCK_FRAMES as u32,
        sample_rate: f64::from(SR),
        timeout: PATIENT,
        group: None,
    };
    let (plugin, audio) = Plugin::start(launch).unwrap();
    (plugin, Hosted::new(audio))
}

#[test]
fn a_plug_in_that_crashes_or_hangs_leaves_the_engine_playing() {
    for (spec, health) in [("test:crash-after:40", Health::Crashed), ("test:hang-after:40", Health::Hung)] {
        let p = project(8.0, false, vec![plugin_device("x")]);
        let key = plugin_devices(&p)[0].key;
        let (mut control, hosted) = start(spec);
        let mut plugins = loopwerk_engine::Plugins::new();
        plugins.insert(key, loopwerk_engine::PluginSlotInfo::default(), Box::new(hosted));
        let mut renderer = Renderer::with_plugins(&p, &AudioPool::new(), SR, plugins).unwrap();
        let mut controller = renderer.controller();
        // The same project without the plug-in, block for block: the effect passes the sound
        // through before it fails, and the engine passes it on without it after.
        let mut dry = Renderer::with_audio(&p, &AudioPool::new(), SR).unwrap();
        let deadline = Instant::now() + PATIENT;
        let mut longest = Duration::ZERO;
        let mut blocks = 0;
        // Until well after it failed (block 40 for the plug-in; more for the engine when a
        // busy test machine makes it miss some).
        while control.health() != health || blocks < 100 {
            assert!(Instant::now() < deadline, "{spec}: still {:?}", control.health());
            let (out, took) = play(&mut renderer, 1);
            let (reference, _) = play(&mut dry, 1);
            assert!(max_diff(&out, &reference) < 1e-6, "{spec}: {}", max_diff(&out, &reference));
            longest = longest.max(took);
            blocks += 1;
        }
        // No block waited much past its deadline (10 ms of audio, 60 % of it for plug-ins),
        // however the plug-in failed; the slack is for a busy test machine.
        assert!(longest < Duration::from_millis(100), "{spec}: a block took {longest:?}");
        // Restarted, it answers again: blocks come back done, so it is not counted as hung.
        let audio = control.restart().unwrap();
        controller.send_plugin(key, Box::new(Hosted::new(audio))).unwrap();
        play(&mut renderer, 20);
        assert_eq!(control.health(), Health::Running, "{spec}");
    }
}

/// A session playing `project` through a renderer driven by the test, with the test catalog.
/// `offline` waits for each block as an offline render does, so what is heard does not depend
/// on how busy the test machine is (the deadline is tested above).
fn session(project: Project, offline: bool) -> (Session, Renderer) {
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    renderer.set_offline(offline);
    let controller = renderer.controller();
    let mut session = Session::new(project);
    session.attach_player(Player::new(controller, None));
    session.set_plugin_env(env());
    (session, renderer)
}

/// Plays blocks while the session looks after its plug-ins, until `done` says so.
fn until(session: &mut Session, renderer: &mut Renderer, what: &str, done: impl Fn(&serde_json::Value) -> bool) {
    let deadline = Instant::now() + PATIENT;
    loop {
        session.sync_plugins();
        play(renderer, 2);
        let status = session.execute("device.plugin", &json!({ "track": 1, "device": 0 })).unwrap();
        if done(&status) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_session_starts_its_projects_plug_ins_and_reports_the_missing_ones() {
    let mut missing = plugin_device("not-installed");
    missing.name = "Gone".into();
    missing.state = Some(vec![1, 2, 3]);
    // A note long enough to still sound however long the plug-in takes to start.
    let p = project(256.0, false, vec![plugin_device("gain"), missing]);
    let (mut session, mut renderer) = session(p.clone(), true);
    session.execute("transport.play", &json!({})).unwrap();
    let notices = session.sync_plugins();
    assert!(notices.iter().any(|n| n.contains("Gone")), "{notices:?}");
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    let status = session.execute("device.plugin", &json!({ "track": 1, "device": 1 })).unwrap();
    assert_eq!(status["status"], "missing");
    let listed = session.execute("track.devices", &json!({ "track": 1 })).unwrap();
    assert_eq!(listed[0]["plugin"]["status"], "running");
    assert_eq!(listed[0]["plugin"]["plugin"]["name"], "Test gain");
    // The missing plug-in keeps its state untouched, to be saved back as it was.
    assert_eq!(session.project().channels[0].devices[1].state, Some(vec![1, 2, 3]));
    assert_eq!(session.project().channels[0].devices, p.channels[0].devices);

    // The plug-in is heard: the track at half its level, once the graph with it plays.
    let (out, _) = play(&mut renderer, 10);
    let mut dry = Renderer::with_audio(&p, &AudioPool::new(), SR).unwrap();
    let (reference, _) = play(&mut dry, 10);
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak(&out) - peak(&reference) * 0.5).abs() < 0.02, "{} vs {}", peak(&out), peak(&reference));

    // Removing the device stops its plug-in; undo starts it again.
    session.execute("device.remove", &json!({ "track": 1, "device": 0 })).unwrap();
    session.sync_plugins();
    let status = session.execute("device.plugin", &json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(status["status"], "missing", "the other device is at 0 now");
    session.execute("edit.undo", &json!({})).unwrap();
    until(&mut session, &mut renderer, "after undo", |s| s["status"] == "running");
}

#[test]
fn a_crashed_plug_in_is_reported_and_restarts_by_command() {
    let p = project(16.0, false, vec![plugin_device("crash-after:40")]);
    let (mut session, mut renderer) = session(p, false);
    session.execute("transport.play", &json!({})).unwrap();
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    until(&mut session, &mut renderer, "crashing", |s| s["status"] == "crashed");
    let error = session.execute("device.openEditor", &json!({ "track": 1, "device": 0 })).unwrap_err();
    assert!(error.0.contains("crashed"), "{error}");
    session.execute("device.restart", &json!({ "track": 1, "device": 0 })).unwrap();
    until(&mut session, &mut renderer, "restarting", |s| s["status"] == "running");
}

#[test]
fn a_plug_in_devices_editor_opens_and_closes_by_command() {
    let p = project(8.0, false, vec![plugin_device("vst3")]);
    let (mut session, mut renderer) = session(p, false);
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    let device = json!({ "track": 1, "device": 0 });
    let opened = session.execute("device.openEditor", &device);
    if cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())) {
        let opened = opened.unwrap();
        assert_eq!((opened["width"].clone(), opened["height"].clone()), (json!(300), json!(200)));
        assert_eq!(session.execute("device.plugin", &device).unwrap()["editor_open"], true);
        session.execute("device.closeEditor", &device).unwrap();
        assert_eq!(session.execute("device.plugin", &device).unwrap()["editor_open"], false);
    } else {
        assert!(opened.is_err());
    }
    // A device that is not a plug-in has no editor.
    let mut other = session.project().clone();
    other.channels[0].devices.push(Device { kind: DeviceKind::Builtin("utility".into()), ..plugin_device("u") });
    let (mut session, _renderer) = self::session(other, false);
    let error = session.execute("device.openEditor", &json!({ "track": 1, "device": 1 })).unwrap_err();
    assert!(error.0.contains("not a plug-in"), "{error}");
}

/// On Windows an editor opened by command floats above the window the session was given: that
/// window owns it. The owner lives on a thread that serves its messages, as the app's window
/// does (an owner that does not would hold up the host making the editor's window).
#[cfg(windows)]
#[test]
fn an_editor_is_owned_by_the_engines_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, FindWindowExW, GW_OWNER, GetMessageW, GetWindow, MSG,
        PostThreadMessageW, WM_QUIT, WS_OVERLAPPEDWINDOW,
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let owner_thread = std::thread::spawn(move || {
        let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
        // SAFETY: a system window class and no title, parent, menu or creation data.
        let owner = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        // SAFETY: no arguments.
        let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        sender.send((owner as usize, thread)).unwrap();
        // SAFETY: a zeroed message is a valid value to be filled.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: any message of this thread, until WM_QUIT.
        while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
            // SAFETY: a message just taken from the queue.
            unsafe { DispatchMessageW(&message) };
        }
        // SAFETY: the window made above, on this thread.
        unsafe { DestroyWindow(owner as _) };
    });
    let (owner, thread) = receiver.recv().unwrap();
    assert_ne!(owner, 0);
    let p = project(8.0, false, vec![plugin_device("vst3")]);
    let (mut session, mut renderer) = session(p, false);
    session.set_editor_owner(Some(owner as u64));
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    // The editor opens in the background: the session's thread (the app's window thread)
    // must not wait for the host, which waits for the owner window while it makes the editor's.
    let asked = std::time::Instant::now();
    let opening = session.execute("device.openEditor", &json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(opening["editor_open"], "opening");
    assert!(asked.elapsed() < Duration::from_millis(500), "{:?}", asked.elapsed());
    until(&mut session, &mut renderer, "opening the editor", |s| s["editor_open"] == true);
    let editor_class: Vec<u16> = "LoopwerkPluginEditor".encode_utf16().chain([0]).collect();
    let mut found = false;
    let mut after = std::ptr::null_mut();
    loop {
        // SAFETY: a class name that outlives the call; the previous window found, or none.
        let window = unsafe { FindWindowExW(std::ptr::null_mut(), after, editor_class.as_ptr(), std::ptr::null()) };
        if window.is_null() {
            break;
        }
        // SAFETY: a window just found.
        found |= unsafe { GetWindow(window, GW_OWNER) } as usize == owner;
        after = window;
    }
    assert!(found, "no editor window owned by the engine's window");
    session.execute("device.closeEditor", &json!({ "track": 1, "device": 0 })).unwrap();
    until(&mut session, &mut renderer, "closing the editor", |s| s["editor_open"] == false);
    // SAFETY: a message without pointers to the owner's thread.
    unsafe { PostThreadMessageW(thread, WM_QUIT, 0, 0) };
    owner_thread.join().unwrap();
}
