//! The engine's side (`loopwerk-plugins`) against the real `loopwerk-plugin-host` process:
//! loading, state, and surviving a host that crashes or hangs, then restarting it.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::{Duration, Instant};

use loopwerk_ipc::Outcome;
use loopwerk_plugins::{HUNG_AFTER, Health, Launch, Plugin, PluginAudio, PluginError, SavedState};

const FRAMES: usize = 64;
const DEADLINE: Duration = Duration::from_millis(2);
const PATIENT: Duration = Duration::from_secs(5);

fn launch(plugin: &str) -> Launch {
    Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_loopwerk-plugin-host")),
        plugin: plugin.into(),
        channels: 2,
        max_frames: FRAMES as u32,
        sample_rate: 48_000.0,
        timeout: PATIENT,
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
    let missing = std::env::temp_dir().join("loopwerk-no-such-plugin.clap");
    let error = Plugin::start(launch(&format!("clap:{}", missing.display()))).err();
    assert!(matches!(&error, Some(PluginError::Host(m)) if m.contains("could not load")), "{error:?}");

    // Any file that is not a plug-in: this test's own source.
    let not_a_plugin = PathBuf::from(file!()).canonicalize().unwrap_or_else(|_| PathBuf::from(file!()));
    let error = Plugin::start(launch(&format!("clap:{}", not_a_plugin.display()))).err();
    assert!(matches!(&error, Some(PluginError::Host(_))), "{error:?}");
}

/// Runs a real CLAP file through the host when `LOOPWERK_TEST_CLAP` names one
/// (`path/to/plugin.clap`, optionally `#plugin.id`); skipped otherwise.
#[test]
fn a_real_clap_file_when_one_is_given() {
    let Ok(spec) = std::env::var("LOOPWERK_TEST_CLAP") else {
        eprintln!("LOOPWERK_TEST_CLAP not set; skipped");
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
    use loopwerk_plugins::offline::render;
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
    use loopwerk_plugins::offline::render_with_state;
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
    let missing = std::env::temp_dir().join("loopwerk-no-such-plugin.vst3");
    let error = Plugin::start(launch(&format!("vst3:{}", missing.display()))).err();
    assert!(matches!(&error, Some(PluginError::Host(_))), "{error:?}");
    let error = Plugin::start(launch("vst3:x.vst3#nothex")).err();
    assert!(matches!(&error, Some(PluginError::Host(m)) if m.contains("class id")), "{error:?}");
}
