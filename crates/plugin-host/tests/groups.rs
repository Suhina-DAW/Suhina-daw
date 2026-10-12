//! Several plug-in instances in one `loopwerk-plugin-host` process (ADR 0006, decision 2):
//! instances in one group share a process, each with its own audio and state; a crash or hang
//! takes the group's process, and restarts bring every instance back into one new process.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::{Duration, Instant};

use loopwerk_ipc::Outcome;
use loopwerk_plugins::{HUNG_AFTER, Health, Launch, Plugin, PluginAudio};

const FRAMES: usize = 64;
const DEADLINE: Duration = Duration::from_millis(2);
const PATIENT: Duration = Duration::from_secs(5);

fn launch(plugin: &str, group: Option<&str>) -> Launch {
    Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_loopwerk-plugin-host")),
        plugin: plugin.into(),
        channels: 2,
        max_frames: FRAMES as u32,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: group.map(str::to_string),
    }
}

fn start(plugin: &str, group: Option<&str>) -> (Plugin, PluginAudio) {
    Plugin::start(launch(plugin, group)).unwrap_or_else(|e| panic!("{plugin}: {e}"))
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

/// Whether a process with id `pid` runs.
fn process_runs(pid: u32) -> bool {
    if cfg!(windows) {
        let filter = format!("PID eq {pid}");
        let output = std::process::Command::new("tasklist").args(["/FI", &filter, "/NH", "/FO", "CSV"]).output();
        output.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    } else {
        PathBuf::from(format!("/proc/{pid}")).exists()
    }
}

#[test]
fn instances_in_a_group_share_a_process_and_keep_apart() {
    let (mut first, mut first_audio) = start("test:gain", Some("share"));
    let (mut second, mut second_audio) = start("test:gain", Some("share"));
    assert_ne!(first.process_id(), 0);
    assert_eq!(first.process_id(), second.process_id());
    first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    for _ in 0..10 {
        assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.25));
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.5));
    }
    assert_eq!(first.save_state().unwrap().data, 0.25f32.to_le_bytes());
    assert_eq!(second.save_state().unwrap().data, 0.5f32.to_le_bytes());
    assert!(first.ping() && second.ping());
    assert_eq!((first.health(), second.health()), (Health::Running, Health::Running));
}

#[test]
fn instances_without_a_group_have_processes_of_their_own() {
    let (first, _a) = start("test:gain", None);
    let (second, _b) = start("test:gain", None);
    assert_ne!(first.process_id(), second.process_id());
    // Another group is another process too.
    let (third, _c) = start("test:gain", Some("own-one"));
    let (fourth, _d) = start("test:gain", Some("own-two"));
    assert_ne!(third.process_id(), fourth.process_id());
}

#[test]
fn a_crash_takes_the_group_and_restarts_bring_both_back_into_one_process() {
    let (mut gain, mut gain_audio) = start("test:gain", Some("crash"));
    gain.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    assert!(gain.params().unwrap().is_empty());
    let (mut crashing, mut crashing_audio) = start("test:crash-after:2", Some("crash"));
    let before = gain.process_id();
    assert_eq!(crashing.process_id(), before);
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.25));
    for _ in 0..2 {
        assert_eq!(block(&mut crashing_audio, PATIENT), (Outcome::Done, 1.0));
    }
    assert_ne!(block(&mut crashing_audio, DEADLINE).0, Outcome::Done);
    wait_for(&mut crashing, Health::Crashed);
    assert_eq!(gain.health(), Health::Crashed);
    assert_ne!(block(&mut gain_audio, DEADLINE).0, Outcome::Done);

    // The first restart starts a new process for the group; the sibling still has lost its
    // process (it is the old one), and its restart joins the new one.
    let mut crashing_audio = crashing.restart().unwrap();
    let after = crashing.process_id();
    assert_ne!(after, before);
    assert_eq!(crashing.health(), Health::Running);
    assert_eq!(gain.health(), Health::Crashed);
    assert!(!gain.ping());
    let mut gain_audio = gain.restart().unwrap();
    assert_eq!(gain.process_id(), after);
    assert_eq!((gain.health(), crashing.health()), (Health::Running, Health::Running));
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.25));
    assert_eq!(block(&mut crashing_audio, PATIENT), (Outcome::Done, 1.0));
}

#[test]
fn a_hung_instance_restarts_its_group_and_the_others_follow() {
    let (mut gain, mut gain_audio) = start("test:gain", Some("hang"));
    let (mut hanging, mut hanging_audio) = start("test:hang-after:1", Some("hang"));
    let before = gain.process_id();
    assert_eq!(block(&mut hanging_audio, PATIENT), (Outcome::Done, 1.0));
    assert_ne!(block(&mut hanging_audio, DEADLINE).0, Outcome::Done);
    for _ in 1..HUNG_AFTER {
        assert_ne!(block(&mut hanging_audio, DEADLINE).0, Outcome::Done);
    }
    // Only the hung instance counts as hung; the other's audio still flows.
    assert_eq!(hanging.health(), Health::Hung);
    assert_eq!(gain.health(), Health::Running);
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.5));

    // Its audio thread is stuck in the process, so its restart replaces the process.
    let mut hanging_audio = hanging.restart().unwrap();
    assert_ne!(hanging.process_id(), before);
    assert_eq!(gain.health(), Health::Crashed);
    let mut gain_audio = gain.restart().unwrap();
    assert_eq!(gain.process_id(), hanging.process_id());
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.5));
    assert_eq!(block(&mut hanging_audio, PATIENT), (Outcome::Done, 1.0));
}

#[test]
fn a_healthy_restart_in_a_group_keeps_the_process() {
    let (mut first, mut first_audio) = start("test:gain", Some("healthy"));
    let (mut second, mut second_audio) = start("test:gain", Some("healthy"));
    let pid = first.process_id();
    first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    let mut first_audio_after = first.restart().unwrap();
    assert_eq!(first.process_id(), pid);
    assert_eq!(block(&mut first_audio_after, PATIENT), (Outcome::Done, 0.25));
    // The old half was let go of; the sibling never noticed.
    assert_ne!(block(&mut first_audio, DEADLINE).0, Outcome::Done);
    assert_eq!(second.health(), Health::Running);
    assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.5));
    assert!(second.ping());
}

#[test]
fn dropping_one_instance_leaves_the_others_and_the_last_ends_the_process() {
    let (first, mut first_audio) = start("test:gain", Some("drop"));
    let (mut second, mut second_audio) = start("test:gain", Some("drop"));
    let pid = second.process_id();
    assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.5));
    drop(first);
    assert_ne!(block(&mut first_audio, DEADLINE).0, Outcome::Done);
    assert_eq!(second.health(), Health::Running);
    assert!(second.ping());
    second.load_state(0.75f32.to_le_bytes().to_vec()).unwrap();
    assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75));
    assert!(process_runs(pid));
    drop(second);
    let deadline = Instant::now() + PATIENT;
    while process_runs(pid) {
        assert!(Instant::now() < deadline, "the process outlived its last instance");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The group starts a new process for the next instance.
    let (third, mut third_audio) = start("test:gain", Some("drop"));
    assert_ne!(third.process_id(), pid);
    assert_eq!(block(&mut third_audio, PATIENT), (Outcome::Done, 0.5));
}

#[test]
fn dropping_a_hung_instance_leaves_the_others_running() {
    let (mut gain, mut gain_audio) = start("test:gain", Some("drop-hung"));
    let (hanging, mut hanging_audio) = start("test:hang-after:0", Some("drop-hung"));
    assert_ne!(block(&mut hanging_audio, DEADLINE).0, Outcome::Done);
    drop(hanging);
    assert_eq!(gain.health(), Health::Running);
    assert!(gain.ping());
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.5));
}

#[test]
fn clap_and_vst3_plugins_load_twice_in_one_process_with_their_own_state() {
    for spec in ["test:clap", "test:vst3"] {
        let group = format!("twice-{spec}");
        let (mut first, mut first_audio) = start(spec, Some(&group));
        let (mut second, mut second_audio) = start(spec, Some(&group));
        assert_eq!(first.process_id(), second.process_id(), "{spec}");
        first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
        second.set_param(0, 0.75).unwrap();
        assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.25), "{spec}");
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75), "{spec}");
        assert_eq!(first.save_state().unwrap().data, 0.25f32.to_le_bytes(), "{spec}");
        assert_eq!(second.save_state().unwrap().data, 0.75f32.to_le_bytes(), "{spec}");
        assert_eq!(first.params().unwrap()[0].value, 0.25, "{spec}");

        // Letting go of one leaves the module loaded for the other, and for a third.
        drop(first);
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75), "{spec}");
        let (third, mut third_audio) = start(spec, Some(&group));
        assert_eq!(third.process_id(), second.process_id(), "{spec}");
        assert_eq!(block(&mut third_audio, PATIENT), (Outcome::Done, 0.5), "{spec}");
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75), "{spec}");
    }
}

#[test]
fn several_editors_open_at_once_in_one_process() {
    let editors_open_here =
        cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()));
    if !editors_open_here {
        eprintln!("no display for editors; skipped");
        return;
    }
    let (mut first, mut first_audio) = start("test:vst3", Some("editors"));
    let (mut second, mut second_audio) = start("test:vst3", Some("editors"));
    assert_eq!(first.open_editor().unwrap(), (300, 200));
    assert_eq!(second.open_editor().unwrap(), (300, 200));
    assert!(first.editor_open() && second.editor_open());
    // The test plug-in's editor turns its own instance's gain to 0.125 when it opens.
    assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.125));
    assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.125));
    first.close_editor().unwrap();
    assert!(!first.editor_open());
    assert!(second.editor_open());
    second.close_editor().unwrap();
    assert!(!second.editor_open());
}
