//! `loopwerk-plugin-host`: the process plug-ins run in, never the engine's (ADR 0006).
//!
//! `loopwerk-plugin-host --control <address>` connects to the engine's control socket, answers
//! its messages (`loopwerk_plugins::protocol`) on the main thread, and once a plug-in is loaded
//! serves audio blocks on a thread of its own through `loopwerk-ipc`. When the control
//! connection closes, the engine is gone and the process exits.
//!
//! Plug-in formats come next; for now the host knows only built-in test processors
//! ([`processors`]), which the engine's tests use to check that it survives a host that
//! crashes or hangs.

mod processors;

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use interprocess::TryClone;
use loopwerk_ipc::Served;
use loopwerk_plugins::protocol::{MAX_LINE, Message, VERSION};
use processors::Processor;

/// How long the audio thread waits for a block before checking whether it should stop.
const POLL: Duration = Duration::from_millis(100);

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

fn run(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stream = loopwerk_plugins::connect(address)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let stop = Arc::new(AtomicBool::new(false));
    let mut processor: Option<Arc<dyn Processor>> = None;
    let mut audio: Option<thread::JoinHandle<()>> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if (&mut reader).take(MAX_LINE as u64 + 1).read_line(&mut line)? == 0 {
            break;
        }
        let reply = match Message::decode(line.trim_end()) {
            Some(Message::Hello { .. }) => Message::Hello { version: VERSION },
            Some(Message::Load { plugin, link }) if processor.is_none() => match load(&plugin, &link, &stop) {
                Ok((loaded, thread)) => {
                    let latency = loaded.latency();
                    processor = Some(loaded);
                    audio = Some(thread);
                    Message::Loaded { latency }
                }
                Err(message) => Message::Error { message },
            },
            Some(Message::Load { .. }) => Message::Error { message: "a plug-in is already loaded".into() },
            Some(Message::Ping { id }) => Message::Pong { id },
            Some(Message::GetState) => match &processor {
                Some(p) => Message::State { data: p.state() },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Some(Message::SetState { data }) => match &processor {
                Some(p) => match p.set_state(&data) {
                    Ok(()) => Message::State { data: p.state() },
                    Err(message) => Message::Error { message },
                },
                None => Message::Error { message: "no plug-in loaded".into() },
            },
            Some(Message::Quit) => break,
            Some(other) => Message::Error { message: format!("unexpected message {other:?}") },
            None => Message::Error { message: "unreadable message".into() },
        };
        let mut out = reply.encode();
        out.push('\n');
        writer.write_all(out.as_bytes())?;
        writer.flush()?;
    }
    stop.store(true, Ordering::Relaxed);
    if let Some(audio) = audio {
        let _ = audio.join();
    }
    Ok(())
}

/// Loads `plugin` and starts serving blocks on the link at `link`.
fn load(
    plugin: &str,
    link: &str,
    stop: &Arc<AtomicBool>,
) -> Result<(Arc<dyn Processor>, thread::JoinHandle<()>), String> {
    let processor = processors::create(plugin)?;
    let mut served = Served::open(link).map_err(|e| e.to_string())?;
    let (audio_processor, stop) = (processor.clone(), stop.clone());
    let thread = thread::Builder::new()
        .name("plug-in audio".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                served.serve(POLL, |input, output| audio_processor.process(input, output));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((processor, thread))
}
