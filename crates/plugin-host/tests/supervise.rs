//! The engine's side (`loopwerk-plugins`) against the real `loopwerk-plugin-host` process:
//! loading, state, and surviving a host that crashes or hangs, then restarting it.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::{Duration, Instant};

use loopwerk_ipc::Outcome;
use loopwerk_plugins::{HUNG_AFTER, Health, Launch, Plugin, PluginAudio, PluginError};

const FRAMES: usize = 64;
const DEADLINE: Duration = Duration::from_millis(2);
const PATIENT: Duration = Duration::from_secs(5);

fn launch(plugin: &str) -> Launch {
    Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_loopwerk-plugin-host")),
        plugin: plugin.into(),
        channels: 2,
        max_frames: FRAMES as u32,
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
    assert_eq!(plugin.save_state().unwrap(), 0.5f32.to_le_bytes());
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
