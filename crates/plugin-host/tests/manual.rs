//! Tools for looking into one installed plug-in by hand, run with `--ignored`: they take the
//! plug-in from `LOOPWERK_PLUGIN` (`vst3:<bundle>` or `clap:<file>`, as the host takes it) and,
//! for a crash, the host from `LOOPWERK_HOST` (a program that runs `loopwerk-plugin-host` with
//! the arguments it is given, such as a debugger that logs the stack when it crashes).
//!
//! ```sh
//! LOOPWERK_PLUGIN="vst3:C:/Program Files/Common Files/VST3/Serum2.vst3" \
//!   cargo test -p loopwerk-plugin-host --test manual -- --ignored --nocapture
//! ```
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::Duration;

use loopwerk_plugins::{Launch, Plugin};

fn launch() -> Option<Launch> {
    let plugin = std::env::var("LOOPWERK_PLUGIN").ok()?;
    let host = std::env::var("LOOPWERK_HOST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_loopwerk-plugin-host")));
    Some(Launch {
        host,
        plugin,
        channels: 2,
        max_frames: 512,
        sample_rate: 48_000.0,
        timeout: Duration::from_secs(120),
        group: None,
    })
}

/// Loads two instances of the plug-in into one host process, one after the other, and
/// reports how long each took (a second instance that takes longer than the first is the
/// plug-in's doing, not the host's).
#[test]
#[ignore = "needs LOOPWERK_PLUGIN; looks into one installed plug-in by hand"]
fn two_instances_load_into_one_process() {
    let Some(launch) = launch() else {
        eprintln!("LOOPWERK_PLUGIN not set; nothing to look into");
        return;
    };
    let launch = Launch { group: Some("manual".into()), ..launch };
    let started = std::time::Instant::now();
    let (first, _first_audio) = Plugin::start(launch.clone()).unwrap_or_else(|e| panic!("first: {e}"));
    eprintln!("first loaded in {:.1} s (latency {})", started.elapsed().as_secs_f64(), first.latency());
    let started = std::time::Instant::now();
    let (second, _second_audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("second: {e}"));
    eprintln!("second loaded in {:.1} s (latency {})", started.elapsed().as_secs_f64(), second.latency());
    assert_eq!(first.process_id(), second.process_id());
}

/// Loads the plug-in, opens its editor in a window of its own, keeps it open for a while and
/// reports whether the host stayed alive.
#[test]
#[ignore = "needs LOOPWERK_PLUGIN; looks into one installed plug-in by hand"]
fn an_installed_plugins_editor_opens() {
    let Some(launch) = launch() else {
        eprintln!("LOOPWERK_PLUGIN not set; nothing to look into");
        return;
    };
    let (mut plugin, _audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("start: {e}"));
    eprintln!("loaded, latency {} frames; opening the editor", plugin.latency());
    match plugin.open_editor() {
        Ok(size) => eprintln!("editor open: {size:?}"),
        Err(e) => panic!("open_editor: {e}"),
    }
    let seconds = std::env::var("LOOPWERK_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(10u64);
    for second in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        let health = plugin.health();
        eprintln!("{second}s: {health:?}, editor open {}", plugin.editor_open());
        assert_eq!(health, loopwerk_plugins::Health::Running, "the host did not stay alive");
    }
    plugin.close_editor().unwrap();
}
