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

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

use interprocess::TryClone;
use loopwerk_ipc::Served;
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
        _ => {
            eprintln!("usage: loopwerk-plugin-host --control <address>");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(address) {
        eprintln!("loopwerk-plugin-host: {e}");
        std::process::exit(1);
    }
}

/// What is loaded.
enum Loaded {
    Clap(clap::ClapPlugin),
    Test { processor: Arc<dyn Processor>, stop: Arc<AtomicBool>, audio: thread::JoinHandle<()> },
}

impl Loaded {
    fn latency(&self) -> u32 {
        match self {
            Self::Clap(plugin) => plugin.latency(),
            Self::Test { processor, .. } => processor.latency(),
        }
    }

    fn state(&mut self) -> Result<Vec<u8>, String> {
        match self {
            Self::Clap(plugin) => plugin.state(),
            Self::Test { processor, .. } => Ok(processor.state()),
        }
    }

    fn set_state(&mut self, data: &[u8]) -> Result<(), String> {
        match self {
            Self::Clap(plugin) => plugin.set_state(data),
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
            Message::GetState => match loaded.as_mut().map(Loaded::state) {
                Some(Ok(data)) => Message::State { data },
                Some(Err(message)) => Message::Error { message },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Message::SetState { data } => match loaded.as_mut() {
                Some(plugin) => match plugin.set_state(&data).and_then(|()| plugin.state()) {
                    Ok(data) => Message::State { data },
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
