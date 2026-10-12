//! `loopwerk-plugin-host`: the process plug-ins run in, never the engine's (ADR 0006).
//!
//! `loopwerk-plugin-host --control <address>` connects to the engine's control socket and
//! answers its messages (`loopwerk_plugins::protocol`) on the main thread, which also serves the
//! plug-in's main-thread callbacks; once a plug-in is loaded, a thread of its own serves audio
//! blocks through `loopwerk-ipc`. When the control connection closes, the engine is gone and
//! the process exits.
//!
//! What it can load: CLAP plug-ins ([`clap`]), the CLAP test plug-in built in for tests
//! ([`test_clap`]), and simple test processors ([`processors`]) that crash or hang on purpose.

mod clap;
mod processors;
mod test_clap;
mod test_vst3;
mod vst3;
mod vst3_events;
mod vst3_params;

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

use interprocess::TryClone;
use loopwerk_ipc::Served;
use loopwerk_plugins::ParamInfo;
use loopwerk_plugins::protocol::{Message, VERSION};
use processors::Processor;

/// How long the audio thread of a test processor waits for a block before checking whether it
/// should stop.
const POLL: Duration = Duration::from_millis(100);
/// How often the main thread looks for plug-in callbacks while no message comes.
const TICK: Duration = Duration::from_millis(10);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let address = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("--control"), Some(address)) => address,
        (Some("--scan"), Some(file)) => scan(file),
        (Some("--scan-worker"), None) => scan_worker(),
        _ => {
            eprintln!("usage: loopwerk-plugin-host --control <address> | --scan <file> | --scan-worker");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(address) {
        eprintln!("loopwerk-plugin-host: {e}");
        std::process::exit(1);
    }
}

/// `--scan <file>`: prints the plug-ins the file holds on one line after
/// [`loopwerk_plugins::scan::SCAN_MARKER`] and exits. A plug-in that crashes or hangs while
/// loading takes this process with it, and the scanner in the engine notices.
///
/// For tests, `test:clap` describes the built-in test plug-in, `test:crash` exits without an
/// answer and `test:hang` never answers.
fn scan(file: &str) -> ! {
    use loopwerk_plugins::scan::{SCAN_MARKER, scan_error_json, scan_json};
    let result = describe(file);
    let (line, code) = match result {
        Ok(plugins) => (scan_json(&plugins), 0),
        Err(message) => (scan_error_json(&message), 1),
    };
    // A plug-in may have printed to stdout while loading; the marker sets this line apart.
    println!("{SCAN_MARKER}{line}");
    std::process::exit(code);
}

/// The plug-ins `file` holds (see [`scan`] for the `test:` names).
fn describe(file: &str) -> Result<Vec<loopwerk_plugins::scan::PluginInfo>, String> {
    match file {
        "test:crash" => std::process::exit(3),
        "test:hang" => loop {
            thread::sleep(Duration::from_secs(60));
        },
        "test:clap" => clap::describe(file),
        "test:vst3" => vst3::describe(file),
        _ if file.to_ascii_lowercase().ends_with(".vst3") => vst3::describe(&format!("vst3:{file}")),
        _ => clap::describe(&format!("clap:{file}")),
    }
}

/// `--scan-worker`: reads one file name per line from standard input and prints each answer on
/// a line after [`loopwerk_plugins::scan::SCAN_MARKER`], until the input ends. The engine's
/// scanner keeps a few of these running so a scan does not start a process per file; one that
/// crashes or hangs on a file is ended and replaced.
fn scan_worker() -> ! {
    use loopwerk_plugins::scan::{SCAN_MARKER, scan_answer};
    use std::io::BufRead;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(file) = line else { break };
        let file = file.trim_end();
        let answer = match describe(file) {
            Ok(plugins) => scan_answer(file, Ok(&plugins)),
            Err(message) => scan_answer(file, Err(&message)),
        };
        println!("{SCAN_MARKER}{answer}");
        let _ = std::io::stdout().flush();
    }
    std::process::exit(0);
}

/// What is loaded.
enum Loaded {
    Clap(clap::ClapPlugin),
    Vst3(vst3::Vst3Plugin),
    Test { processor: Arc<dyn Processor>, stop: Arc<AtomicBool>, audio: thread::JoinHandle<()> },
}

impl Loaded {
    fn latency(&self) -> u32 {
        match self {
            Self::Clap(plugin) => plugin.latency(),
            Self::Vst3(plugin) => plugin.latency(),
            Self::Test { processor, .. } => processor.latency(),
        }
    }

    fn params(&mut self) -> Result<Vec<ParamInfo>, String> {
        match self {
            Self::Clap(plugin) => Ok(plugin.params()),
            Self::Vst3(plugin) => Ok(plugin.params()),
            Self::Test { .. } => Ok(Vec::new()),
        }
    }

    fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        match self {
            Self::Clap(plugin) => plugin.set_param(id, value),
            Self::Vst3(plugin) => plugin.set_param(id, value),
            Self::Test { .. } => Err(format!("no parameter {id}")),
        }
    }

    /// The component's state and, for VST3, the controller's.
    fn state(&mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
        match self {
            Self::Clap(plugin) => Ok((plugin.state()?, None)),
            Self::Vst3(plugin) => Ok((plugin.state()?, plugin.controller_state())),
            Self::Test { processor, .. } => Ok((processor.state(), None)),
        }
    }

    fn set_state(&mut self, data: &[u8], controller: Option<&[u8]>) -> Result<(), String> {
        match self {
            Self::Clap(plugin) => plugin.set_state(data),
            Self::Vst3(plugin) => plugin.set_state(data, controller),
            Self::Test { processor, .. } => processor.set_state(data),
        }
    }

    fn tick(&mut self) {
        if let Self::Clap(plugin) = self {
            plugin.tick();
        }
    }

    /// Stops the audio thread (a hung one is left behind; the process is about to exit).
    fn unload(self) {
        match self {
            Self::Clap(plugin) => drop(plugin),
            Self::Vst3(plugin) => drop(plugin),
            Self::Test { stop, audio, .. } => {
                stop.store(true, Ordering::Release);
                if audio.is_finished() {
                    let _ = audio.join();
                }
            }
        }
    }
}

fn load(plugin: &str, link: &str, sample_rate: f64) -> Result<Loaded, String> {
    let served = Served::open(link).map_err(|e| e.to_string())?;
    if plugin.starts_with("clap:") || plugin == "test:clap" {
        return clap::ClapPlugin::load(plugin, served, sample_rate).map(Loaded::Clap);
    }
    if plugin.starts_with("vst3:") || plugin == "test:vst3" {
        return vst3::Vst3Plugin::load(plugin, served, sample_rate).map(Loaded::Vst3);
    }
    let processor = processors::create(plugin)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (audio_processor, audio_stop, mut served) = (processor.clone(), stop.clone(), served);
    let audio = thread::Builder::new()
        .name("plug-in audio".into())
        .spawn(move || {
            while !audio_stop.load(Ordering::Acquire) {
                served.serve(POLL, |input, output| audio_processor.process(input, output));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Loaded::Test { processor, stop, audio })
}

fn run(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stream = loopwerk_plugins::connect(address)?;
    let mut writer = stream.try_clone()?;
    let messages = loopwerk_plugins::read_lines(stream);
    let mut loaded: Option<Loaded> = None;
    loop {
        let message = match messages.recv_timeout(TICK) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(loaded) = &mut loaded {
                    loaded.tick();
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let reply = match message {
            Message::Hello { .. } => Message::Hello { version: VERSION },
            Message::Load { plugin, link, sample_rate } if loaded.is_none() => {
                match load(&plugin, &link, sample_rate) {
                    Ok(fresh) => {
                        let latency = fresh.latency();
                        loaded = Some(fresh);
                        Message::Loaded { latency }
                    }
                    Err(message) => Message::Error { message },
                }
            }
            Message::Load { .. } => Message::Error { message: "a plug-in is already loaded".into() },
            Message::Ping { id } => Message::Pong { id },
            Message::GetParams => match loaded.as_mut().map(Loaded::params) {
                Some(Ok(params)) => Message::Params { params },
                Some(Err(message)) => Message::Error { message },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Message::SetParam { id, value } => match loaded.as_mut().map(|p| p.set_param(id, value)) {
                Some(Ok(param)) => Message::Param { param },
                Some(Err(message)) => Message::Error { message },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Message::GetState => match loaded.as_mut().map(Loaded::state) {
                Some(Ok((data, controller))) => Message::State { data, controller },
                Some(Err(message)) => Message::Error { message },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Message::SetState { data, controller } => match loaded.as_mut() {
                Some(plugin) => match plugin.set_state(&data, controller.as_deref()).and_then(|()| plugin.state()) {
                    Ok((data, controller)) => Message::State { data, controller },
                    Err(message) => Message::Error { message },
                },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Message::Quit => break,
            other => Message::Error { message: format!("unexpected message {other:?}") },
        };
        let mut out = reply.encode();
        out.push('\n');
        writer.write_all(out.as_bytes())?;
        writer.flush()?;
    }
    if let Some(loaded) = loaded {
        loaded.unload();
    }
    Ok(())
}
