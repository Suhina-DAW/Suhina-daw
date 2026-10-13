//! The engine's side (`suhina-plugins`) against the real `suhina-plugin-host` process:
//! loading, state, and surviving a host that crashes or hangs, then restarting it.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use suhina_ipc::Outcome;
use suhina_plugins::{HUNG_AFTER, Health, Launch, Plugin, PluginAudio, PluginError, SavedState};

const FRAMES: usize = 64;
const DEADLINE: Duration = Duration::from_millis(2);
const PATIENT: Duration = Duration::from_secs(5);

fn launch(plugin: &str) -> Launch {
    Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: plugin.into(),
        channels: 2,
        max_frames: FRAMES as u32,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: None,
    }
}

fn start(plugin: &str) -> (Plugin, PluginAudio) {
    Plugin::start(launch(plugin)).unwrap_or_else(|e| panic!("{plugin}: {e}"))
}

/// Runs one block of ones and returns the outcome and the first output sample.
fn block(audio: &mut PluginAudio, timeout: Duration) -> (Outcome, f32) {
    let mut output = [9.0f32; FRAMES * 2];
    let outcome = audio.process(&[1.0; FRAMES * 2], &mut output, timeout);
    (outcome, output[0])
}

fn wait_for(plugin: &mut Plugin, health: Health) {
    let deadline = Instant::now() + PATIENT;
    while plugin.health() != health {
        assert!(Instant::now() < deadline, "never became {health:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn loads_processes_and_keeps_state() {
    let (mut plugin, mut audio) = start("test:gain");
    assert_eq!(plugin.latency(), 0);
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.5));
    assert!(plugin.ping());
    assert_eq!(plugin.save_state().unwrap().data, 0.5f32.to_le_bytes());
    plugin.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25));
    assert!(matches!(plugin.load_state(vec![1, 2, 3]), Err(PluginError::Host(_))));
    assert_eq!(plugin.health(), Health::Running);
}

#[test]
fn a_restart_comes_back_with_the_last_state() {
    let (mut plugin, mut audio) = start("test:gain");
    plugin.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    let mut audio_after = plugin.restart().unwrap();
    assert_eq!(block(&mut audio_after, PATIENT), (Outcome::Done, 0.25));
    // The old half belongs to a process that is gone: it never answers again.
    assert_ne!(block(&mut audio, DEADLINE).0, Outcome::Done);
}

#[test]
fn a_crash_is_noticed_and_a_restart_recovers() {
    let (mut plugin, mut audio) = start("test:crash-after:3");
    for _ in 0..3 {
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 1.0));
    }
    let started = Instant::now();
    assert_eq!(block(&mut audio, DEADLINE).0, Outcome::Missed);
    assert_eq!(block(&mut audio, DEADLINE).0, Outcome::Busy);
    assert!(started.elapsed() < Duration::from_millis(50), "{:?}", started.elapsed());
    wait_for(&mut plugin, Health::Crashed);
    assert!(!plugin.ping());

    let mut audio = plugin.restart().unwrap();
    assert_eq!(plugin.health(), Health::Running);
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 1.0));
}

#[test]
fn a_hang_is_noticed_and_a_restart_recovers() {
    let (mut plugin, mut audio) = start("test:hang-after:3");
    for _ in 0..3 {
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 1.0));
    }
    assert_eq!(block(&mut audio, DEADLINE).0, Outcome::Missed);
    for _ in 1..HUNG_AFTER {
        assert_eq!(block(&mut audio, DEADLINE).0, Outcome::Busy);
    }
    // The audio thread is stuck but the process is alive and its control thread still answers;
    // only the count of unanswered blocks tells.
    assert_eq!(plugin.health(), Health::Hung);
    assert!(plugin.ping());

    let mut audio = plugin.restart().unwrap();
    assert_eq!(plugin.health(), Health::Running);
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 1.0));
}

#[test]
fn start_failures_are_errors() {
    assert!(matches!(Plugin::start(launch("no-such-plugin")), Err(PluginError::Host(m)) if m.contains("unknown")));
    let missing = Launch { host: PathBuf::from("no-such-host-executable"), ..launch("test:gain") };
    assert!(matches!(Plugin::start(missing), Err(PluginError::Io(_))));
}

#[test]
fn a_clap_plugin_processes_reports_latency_and_keeps_state() {
    let (mut plugin, mut audio) = start("test:clap");
    assert_eq!(plugin.latency(), 16);
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.5));
    assert_eq!(plugin.save_state().unwrap().data, 0.5f32.to_le_bytes());
    plugin.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25));
    assert!(matches!(plugin.load_state(vec![1, 2]), Err(PluginError::Host(_))));

    let mut audio = plugin.restart().unwrap();
    assert_eq!(plugin.latency(), 16);
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25));
    assert!(plugin.ping());
}

#[test]
fn clap_files_that_are_missing_or_not_plugins_are_errors() {
    let missing = std::env::temp_dir().join("suhina-no-such-plugin.clap");
    let error = Plugin::start(launch(&format!("clap:{}", missing.display()))).err();
    assert!(matches!(&error, Some(PluginError::Host(m)) if m.contains("could not load")), "{error:?}");

    // Any file that is not a plug-in: this test's own source.
    let not_a_plugin = PathBuf::from(file!()).canonicalize().unwrap_or_else(|_| PathBuf::from(file!()));
    let error = Plugin::start(launch(&format!("clap:{}", not_a_plugin.display()))).err();
    assert!(matches!(&error, Some(PluginError::Host(_))), "{error:?}");
}

/// Runs a real CLAP file through the host when `SUHINA_TEST_CLAP` names one
/// (`path/to/plugin.clap`, optionally `#plugin.id`); skipped otherwise.
#[test]
fn a_real_clap_file_when_one_is_given() {
    let Ok(spec) = std::env::var("SUHINA_TEST_CLAP") else {
        eprintln!("SUHINA_TEST_CLAP not set; skipped");
        return;
    };
    let (mut plugin, mut audio) = start(&format!("clap:{spec}"));
    let mut done = 0;
    for _ in 0..100 {
        if block(&mut audio, PATIENT).0 == Outcome::Done {
            done += 1;
        }
    }
    assert_eq!(done, 100);
    let state = plugin.save_state();
    eprintln!("{spec}: latency {}, state {:?} bytes", plugin.latency(), state.as_ref().map(|s| s.data.len()));
    if let Ok(state) = state {
        plugin.load_state(state).unwrap();
    }
    assert!(plugin.ping());
}

#[test]
fn offline_runs_line_the_output_up_with_the_input_and_ring_out() {
    use suhina_plugins::offline::render;
    // 1500 frames of a ramp, through the test CLAP gain (0.5, reports 16 frames of latency).
    let input: Vec<f32> = (0..1_500).flat_map(|n| [n as f32, -(n as f32)]).collect();
    let rendered = render(launch("test:clap"), &input, 100).unwrap();
    assert_eq!(rendered.latency, 16);
    assert_eq!(rendered.samples.len(), (1_500 + 100) * 2);
    // The test plug-in does not really delay, so taking its reported latency out moves the
    // output 16 frames early: frame n holds input frame n + 16.
    assert_eq!(rendered.samples[0], 16.0 * 0.5);
    assert_eq!(rendered.samples[1], -16.0 * 0.5);
    assert_eq!(rendered.samples[2 * 1_000], 1_016.0 * 0.5);
    assert!(rendered.samples[2 * 1_484..].iter().all(|&s| s == 0.0));

    // With a state first: one the plug-in takes, and one it refuses (it runs with its default).
    use suhina_plugins::offline::render_with_state;
    let quieter =
        render_with_state(launch("test:vst3"), Some(0.25f32.to_le_bytes().to_vec().into()), &input, 0).unwrap();
    assert_eq!(quieter.state_refused, None);
    assert!(quieter.samples.iter().any(|s| *s != 0.0));
    let default = render(launch("test:vst3"), &input, 0).unwrap();
    for (q, d) in quieter.samples.iter().zip(&default.samples) {
        assert!((q * 2.0 - d).abs() < 1e-6, "{q} {d}");
    }
    let refused = render_with_state(launch("test:vst3"), Some(vec![1].into()), &input, 0).unwrap();
    assert!(refused.state_refused.is_some());
    assert_eq!(refused.samples, default.samples);

    let crashing = render(launch("test:crash-after:2"), &input, 0);
    assert!(matches!(crashing, Err(PluginError::Timeout(_))), "{crashing:?}");
}

#[test]
fn a_vst3_plugin_processes_reports_latency_and_keeps_state() {
    let (mut plugin, mut audio) = start("test:vst3");
    assert_eq!(plugin.latency(), 32);
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.5));
    assert_eq!(plugin.save_state().unwrap().data, 0.5f32.to_le_bytes());
    plugin.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25));
    assert!(matches!(plugin.load_state(vec![1, 2]), Err(PluginError::Host(_))));

    let mut audio = plugin.restart().unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25));

    // The controller's own state goes with the component's, and survives a restart.
    let saved = SavedState { data: 0.75f32.to_le_bytes().to_vec(), controller: Some(vec![9, 8, 7]) };
    plugin.load_state(saved.clone()).unwrap();
    assert_eq!(plugin.save_state().unwrap(), saved);
    let mut audio = plugin.restart().unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.75));
    assert_eq!(plugin.save_state().unwrap(), saved);
}

#[test]
fn a_vst3_plugin_reads_the_vst2_bank_of_the_plugin_it_replaces() {
    let (mut plugin, mut audio) = start("test:vst3");
    // A chunk bank as a project's VST2 state becomes: the header, then the chunk (the gain).
    let mut bank = b"CcnK".to_vec();
    bank.extend_from_slice(&(160 - 8 + 4u32).to_be_bytes());
    bank.extend_from_slice(b"FBCh");
    bank.resize(156, 0);
    bank.extend_from_slice(&4u32.to_be_bytes());
    bank.extend_from_slice(&0.125f32.to_le_bytes());
    plugin.load_state(bank).unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.125));
    // From then on it saves its own state.
    assert_eq!(plugin.save_state().unwrap().data, 0.125f32.to_le_bytes());
}

#[test]
fn a_bank_of_programs_is_set_as_parameters() {
    let (mut plugin, mut audio) = start("test:vst3");
    // A bank of one program whose first parameter (the gain, id 0) is 0.125; the component
    // does not read such a bank itself, so the value reaches it as a parameter change.
    let mut bank = b"CcnK".to_vec();
    bank.extend_from_slice(&[0; 4]);
    bank.extend_from_slice(b"FxBk");
    for word in [2u32, 7, 1, 1, 0] {
        bank.extend_from_slice(&word.to_be_bytes());
    }
    bank.resize(156, 0);
    bank.extend_from_slice(b"CcnK");
    bank.extend_from_slice(&(4u32 * 5 + 28 + 4).to_be_bytes());
    bank.extend_from_slice(b"FxCk");
    for word in [1u32, 7, 1, 1] {
        bank.extend_from_slice(&word.to_be_bytes());
    }
    bank.extend_from_slice(&[0; 28]);
    bank.extend_from_slice(&0.125f32.to_be_bytes());
    plugin.load_state(bank.clone()).unwrap();
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.125));
    // A bank with more parameters than the plug-in has ids for is not set by number.
    let mut wider = bank.clone();
    wider.extend_from_slice(&0.5f32.to_be_bytes());
    let size = 4u32 * 5 + 28 + 8;
    wider[156 + 4..156 + 8].copy_from_slice(&size.to_be_bytes());
    wider[156 + 24..156 + 28].copy_from_slice(&2u32.to_be_bytes());
    assert!(matches!(plugin.load_state(wider), Err(PluginError::Host(_))));
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.125));
}

#[test]
fn vst3_files_that_are_missing_or_not_plugins_are_errors() {
    let missing = std::env::temp_dir().join("suhina-no-such-plugin.vst3");
    let error = Plugin::start(launch(&format!("vst3:{}", missing.display()))).err();
    assert!(matches!(&error, Some(PluginError::Host(_))), "{error:?}");
    let error = Plugin::start(launch("vst3:x.vst3#nothex")).err();
    assert!(matches!(&error, Some(PluginError::Host(m)) if m.contains("class id")), "{error:?}");
}

#[test]
fn notes_and_controllers_reach_clap_and_vst3_plugins() {
    use suhina_plugins::{Event, EventKind};
    for spec in ["test:clap", "test:vst3"] {
        let (_plugin, mut audio) = start(spec);
        // A note at frame 10 adds its velocity there, on the left channel only.
        let mut output = [9.0f32; FRAMES * 2];
        let notes = [Event::note_on(10, 0, 60, 0.75), Event::note_off(20, 0, 60, 0.0)];
        assert_eq!(audio.process_with_events(&[0.0; FRAMES * 2], &notes, &mut output, PATIENT), Outcome::Done);
        assert_eq!((output[20], output[21], output[18], output[22]), (0.75, 0.0, 0.0, 0.0), "{spec}");
        // Controller 7 at full sets the gain to 1 (it starts at 0.5).
        let volume = [Event { frame: 0, note: -1, kind: EventKind::Controller { channel: 0, number: 7, value: 1.0 } }];
        assert_eq!(audio.process_with_events(&[1.0; FRAMES * 2], &volume, &mut output, PATIENT), Outcome::Done);
        assert_eq!(output[0], 1.0, "{spec}");
        // Blocks without events play as before.
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 1.0), "{spec}");
    }
}

#[test]
fn an_offline_run_plays_notes_at_their_frames() {
    use suhina_plugins::Event;
    use suhina_plugins::offline::render_with_events;
    // 2000 frames of silence through the VST3 test plug-in, with notes in the first and the
    // fourth block (blocks are 512 frames offline).
    let input = vec![0.0f32; 2_000 * 2];
    let notes = [Event::note_on(100, 0, 60, 0.5), Event::note_on(1_600, 0, 64, 0.25)];
    let rendered = render_with_events(launch("test:vst3"), None, &input, &notes, 0).unwrap();
    // The output is lined up with the input by taking out the 32 frames of latency the plug-in
    // reports; it does not really delay, so a note at frame f lands at f - 32.
    let at = |frame: usize| rendered.samples[(frame - 32) * 2];
    assert_eq!((at(100), at(1_600)), (0.5, 0.25));
    let total: f32 = rendered.samples.iter().sum();
    assert_eq!(total, 0.75);
}

/// Asks the editor's window to close, as the user does with its close button.
#[cfg(windows)]
fn close_editor_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    let class: Vec<u16> = "SuhinaPluginEditor".encode_utf16().chain([0]).collect();
    // SAFETY: a class name that outlives the call; any title.
    let window = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    assert!(!window.is_null(), "no editor window");
    // SAFETY: a window found just now; WM_CLOSE carries no pointers.
    unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
}

/// Asks the editor's window to close, as the user does with its close button: the window
/// manager's WM_DELETE_WINDOW message, sent to the window whose class is the editor's.
#[cfg(target_os = "linux")]
fn close_editor_window() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask};
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let atom = |name: &[u8]| connection.intern_atom(false, name).unwrap().reply().unwrap().atom;
    let (protocols, delete) = (atom(b"WM_PROTOCOLS"), atom(b"WM_DELETE_WINDOW"));
    let children = connection.query_tree(root).unwrap().reply().unwrap().children;
    let window = children
        .into_iter()
        .find(|w| {
            let class = connection.get_property(false, *w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64);
            class.ok().and_then(|c| c.reply().ok()).is_some_and(|c| c.value.starts_with(b"suhina-plugin-editor"))
        })
        .unwrap_or_else(|| panic!("no editor window"));
    let message = ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]);
    connection.send_event(false, window, EventMask::NO_EVENT, message).unwrap();
    connection.flush().unwrap();
}

#[cfg(not(any(windows, target_os = "linux")))]
fn close_editor_window() {}

/// Whether editors can open here: on Windows, and on Linux with an X server.
fn editors_open_here() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()))
}

#[test]
fn an_editor_opens_in_a_window_and_closes() {
    let (mut plugin, mut audio) = start("test:vst3");
    let opened = plugin.open_editor();
    if editors_open_here() {
        assert_eq!(opened.unwrap(), (300, 200));
        assert!(plugin.editor_open());
        // Opening it again gives the same editor; audio keeps flowing, with the gain the
        // editor set.
        assert_eq!(plugin.open_editor().unwrap(), (300, 200));
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.125));
        plugin.close_editor().unwrap();
        assert!(!plugin.editor_open());
        // And it opens again after being closed; when the user closes its window, the host
        // says so unasked.
        assert_eq!(plugin.open_editor().unwrap(), (300, 200));
        close_editor_window();
        let deadline = Instant::now() + PATIENT;
        while plugin.editor_open() {
            assert!(Instant::now() < deadline, "the closed window was not reported");
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        assert!(matches!(opened, Err(PluginError::Host(_))), "{opened:?}");
    }
    // An edit made in the editor (the test plug-in's turns the gain to 0.125 when it opens)
    // reaches the processor and the saved state.
    if editors_open_here() {
        let (mut edited, mut audio) = start("test:vst3");
        edited.open_editor().unwrap();
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.125));
        assert_eq!(edited.save_state().unwrap().data, 0.125f32.to_le_bytes());
    }
    // A plug-in without an editor says so.
    let (mut plain, _audio) = start("test:clap");
    assert!(matches!(plain.open_editor(), Err(PluginError::Host(_))));
    plain.close_editor().unwrap();
}

/// An editor asked for embedded opens as a child inside the window the engine names, hidden
/// until placed; the engine learns its size, and closing it takes the child away.
#[test]
fn an_editor_embeds_in_the_window_the_engine_names() {
    use common::{ParentWindow, soon};
    use suhina_plugins::Placement;
    if !common::editors_open_here() {
        eprintln!("no display for editors; skipped");
        return;
    }
    let parent = ParentWindow::open();
    let (mut plugin, mut audio) = start("test:vst3");
    assert_eq!(plugin.open_editor_embedded(parent.id()).unwrap(), (300, 200));
    assert!(plugin.editor_open());
    assert_eq!(plugin.editor_size(), Some((300, 200)));
    assert!(soon(PATIENT, || parent.editor_children() == 1), "no editor child in the engine's window");
    // Placing it is never waited for; the host takes it between messages.
    let shown = Placement { x: 8, y: 40, clip_x: 0, clip_y: 40, clip_width: 400, clip_height: 260 };
    assert!(plugin.place_editor(shown).unwrap());
    assert!(plugin.place_editor(Placement::HIDDEN).unwrap());
    // Audio keeps flowing, with the gain the editor set when it opened.
    assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.125));
    plugin.close_editor().unwrap();
    assert!(!plugin.editor_open());
    assert_eq!(plugin.editor_size(), None);
    assert!(soon(PATIENT, || parent.editor_children() == 0), "the editor's child window stayed");
    // A window that is not one to embed in is an error, not a crash.
    assert!(matches!(plugin.open_editor_embedded(0), Err(PluginError::Host(_))));
    assert!(!plugin.editor_open());
}

#[test]
fn parameter_events_automate_a_parameter_from_their_frame() {
    use suhina_plugins::Event;
    for spec in ["test:clap", "test:vst3"] {
        let (_plugin, mut audio) = start(spec);
        let mut output = [9.0f32; FRAMES * 2];
        // The test plug-ins take a block's last value for the whole block.
        let automation = [Event::param(10, 0, 0.75)];
        assert_eq!(audio.process_with_events(&[1.0; FRAMES * 2], &automation, &mut output, PATIENT), Outcome::Done);
        assert_eq!(output[0], 0.75, "{spec}");
        // It stays until the next value; an unknown parameter changes nothing.
        let unknown = [Event::param(0, 99, 0.1)];
        assert_eq!(audio.process_with_events(&[1.0; FRAMES * 2], &unknown, &mut output, PATIENT), Outcome::Done);
        assert_eq!(output[0], 0.75, "{spec}");
    }
}

#[test]
fn parameters_are_listed_set_and_kept_over_a_restart() {
    for spec in ["test:clap", "test:vst3"] {
        let (mut plugin, mut audio) = start(spec);
        let params = plugin.params().unwrap();
        let [gain] = params.as_slice() else { panic!("{spec}: {params:?}") };
        assert_eq!((gain.id, gain.name.as_str(), gain.value, gain.default), (0, "Gain", 0.5, 0.5), "{spec}");
        assert_eq!((gain.min, gain.max, gain.automatable), (0.0, 1.0, true), "{spec}");

        let set = plugin.set_param(0, 0.25).unwrap();
        assert_eq!(set.value, 0.25, "{spec}");
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25), "{spec}");
        // Values are kept within the range; unknown parameters are errors.
        assert_eq!(plugin.set_param(0, 7.0).unwrap().value, 1.0, "{spec}");
        assert!(matches!(plugin.set_param(99, 0.5), Err(PluginError::Host(_))), "{spec}");
        plugin.set_param(0, 0.25).unwrap();
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25), "{spec}");

        // A restart comes back with the value set, though no state was saved since.
        let mut audio = plugin.restart().unwrap();
        assert_eq!(block(&mut audio, PATIENT), (Outcome::Done, 0.25), "{spec}");
    }
    let (mut plain, _audio) = start("test:gain");
    assert!(plain.params().unwrap().is_empty());
}
