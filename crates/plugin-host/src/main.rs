//! `loopwerk-plugin-host`: the process plug-ins run in, never the engine's (ADR 0006).
//!
//! `loopwerk-plugin-host --control <address>` connects to the engine's control socket and
//! answers its messages (`loopwerk_plugins::protocol`) on the main thread, which also serves the
//! plug-ins' main-thread callbacks and their editors. It runs any number of plug-in instances,
//! each loaded and unloaded on its own and named by the number the engine gave it; each has
//! its own link and a thread of its own that serves its audio blocks through `loopwerk-ipc`.
//! When the control connection closes, the engine is gone and the process exits.
//!
//! What it can load: CLAP plug-ins ([`clap`]), the CLAP test plug-in built in for tests
//! ([`test_clap`]), and simple test processors ([`processors`]) that crash or hang on purpose.

mod clap;
mod processors;
mod run_loop;
mod test_clap;
mod test_vst3;
mod vst3;
mod vst3_editor;
mod vst3_events;
mod vst3_message;
mod vst3_params;
mod window;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::{Duration, Instant};

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
/// The same while an editor is open where the main thread cannot wait for its window's events
/// (X11): often enough for input and drawing to feel immediate.
#[cfg(not(windows))]
const EDITOR_TICK: Duration = Duration::from_millis(1);
/// How long unloading waits for an instance's audio thread to stop; one still running then is
/// stuck in the plug-in, and the instance is left behind rather than taking the process with it.
const UNLOAD_WAIT: Duration = Duration::from_secs(1);
/// Most instances one process runs.
const MAX_INSTANCES: usize = 1024;

/// Gives the calling thread, an instance's audio thread, the highest priority the system
/// allows it (time-critical on Windows), so it answers the engine's blocks as soon as they
/// come: the engine waits only until each block's deadline (ADR 0006). Where that is not
/// allowed (Linux without the right), the thread runs at its normal priority.
pub(crate) fn real_time_thread() {
    // `LOOPWERK_NO_REALTIME=1` leaves the thread at its normal priority, to compare.
    if std::env::var_os("LOOPWERK_NO_REALTIME").is_some_and(|v| v == "1") {
        return;
    }
    let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max);
}

/// Asks an audio thread to stop and waits up to `wait` for it to end; whether it did (or there
/// was none).
pub(crate) fn audio_stopped<T>(stop: &AtomicBool, audio: Option<&thread::JoinHandle<T>>, wait: Duration) -> bool {
    stop.store(true, Ordering::Release);
    let deadline = Instant::now() + wait;
    loop {
        if audio.is_none_or(thread::JoinHandle::is_finished) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

/// Sets the main thread up as a plug-in's editor expects a host's window thread to be: on
/// Windows with OLE (and so COM, apartment threaded) initialised, which editors use for drag
/// and drop, the clipboard and text services, and which some crash without (Serum 2, Z3TA+ 2:
/// a null object from a COM call made before `OleInitialize`).
fn prepare_main_thread() {
    #[cfg(windows)]
    {
        // SAFETY: called once, on the main thread, before any window or plug-in; a null
        // reserved argument as the function requires.
        let _ = unsafe { windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null_mut()) };
    }
}

fn main() {
    prepare_main_thread();
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

    fn open_editor(&mut self, mode: window::Mode) -> Result<window::Size, String> {
        match self {
            Self::Clap(plugin) => plugin.open_editor(mode),
            Self::Vst3(plugin) => plugin.open_editor(mode),
            Self::Test { .. } => Err("the plug-in has no editor".into()),
        }
    }

    /// Puts an embedded editor where the engine says.
    fn place_editor(&self, placement: window::Placement) {
        match self {
            Self::Clap(plugin) => plugin.place_editor(placement),
            Self::Vst3(plugin) => plugin.place_editor(placement),
            Self::Test { .. } => {}
        }
    }

    /// A size the open editor got since this was last asked, to tell the engine.
    fn editor_resized(&mut self) -> Option<window::Size> {
        match self {
            Self::Clap(plugin) => plugin.editor_resized(),
            Self::Vst3(plugin) => plugin.editor_resized(),
            Self::Test { .. } => None,
        }
    }

    fn close_editor(&mut self) {
        match self {
            Self::Clap(plugin) => plugin.close_editor(),
            Self::Vst3(plugin) => plugin.close_editor(),
            Self::Test { .. } => {}
        }
    }

    /// Keeps an open editor in step with its window; whether the user closed it.
    fn editor_tick(&mut self) -> bool {
        match self {
            Self::Clap(plugin) => plugin.editor_tick(),
            Self::Vst3(plugin) => plugin.editor_tick(),
            Self::Test { .. } => false,
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

    /// Closes the editor, stops the audio thread and lets go of the plug-in. An audio thread
    /// that does not stop is stuck in the plug-in's code: the plug-in is then left as it is
    /// (never deactivated or freed under it), so the other instances go on.
    fn unload(self) {
        match self {
            Self::Clap(mut plugin) => {
                plugin.close_editor();
                if plugin.stop_audio(UNLOAD_WAIT) {
                    drop(plugin);
                } else {
                    std::mem::forget(plugin);
                }
            }
            Self::Vst3(mut plugin) => {
                plugin.close_editor();
                if plugin.stop_audio(UNLOAD_WAIT) {
                    drop(plugin);
                } else {
                    std::mem::forget(plugin);
                }
            }
            Self::Test { stop, audio, .. } => {
                if audio_stopped(&stop, Some(&audio), UNLOAD_WAIT) {
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
            real_time_thread();
            while !audio_stop.load(Ordering::Acquire) {
                served.serve(POLL, |input, output| audio_processor.process(input, output));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Loaded::Test { processor, stop, audio })
}

/// Writes one message to the engine.
fn send(writer: &mut impl Write, message: &Message) -> std::io::Result<()> {
    let mut out = message.encode();
    out.push('\n');
    writer.write_all(out.as_bytes())?;
    writer.flush()
}

/// The engine's messages, on Windows passed through a thread that wakes the main thread when
/// one comes, so the main thread can wait for its windows' messages instead of polling them.
#[cfg(windows)]
fn wakeful(messages: std::sync::mpsc::Receiver<Message>) -> std::sync::mpsc::Receiver<Message> {
    let waker = window::Waker::current();
    let (sender, receiver) = std::sync::mpsc::channel();
    let _forwarder = thread::Builder::new().name("control messages".into()).spawn(move || {
        for message in messages {
            if sender.send(message).is_err() {
                return;
            }
            waker.wake();
        }
        // The engine is gone: wake the main thread to notice.
        waker.wake();
    });
    receiver
}

#[cfg(not(windows))]
fn wakeful(messages: std::sync::mpsc::Receiver<Message>) -> std::sync::mpsc::Receiver<Message> {
    messages
}

/// The next message from the engine, or a timeout after the main thread has had something
/// else to do: on Windows a window message (an editor's input, painting, a plug-in's timer),
/// which is then served at once; elsewhere a tick.
#[cfg(windows)]
fn next_message(messages: &std::sync::mpsc::Receiver<Message>) -> Result<Message, RecvTimeoutError> {
    match messages.try_recv() {
        Ok(message) => Ok(message),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            window::wait(TICK);
            messages.try_recv().map_err(|e| match e {
                std::sync::mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                std::sync::mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        }
    }
}

#[cfg(not(windows))]
fn next_message(messages: &std::sync::mpsc::Receiver<Message>) -> Result<Message, RecvTimeoutError> {
    messages.recv_timeout(if window::any_open() { EDITOR_TICK } else { TICK })
}

/// The instances loaded, by the number the engine gave each.
type Instances = BTreeMap<u32, Loaded>;

fn run(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stream = loopwerk_plugins::connect(address)?;
    let mut writer = stream.try_clone()?;
    let messages = wakeful(loopwerk_plugins::read_lines(stream));
    let mut instances = Instances::new();
    loop {
        // Between messages: the plug-ins' main-thread callbacks and their editors' windows.
        window::pump();
        for (instance, loaded) in &mut instances {
            loaded.tick();
            if loaded.editor_tick() {
                send(&mut writer, &Message::EditorClosed { instance: *instance })?;
            } else if let Some(size) = loaded.editor_resized() {
                send(
                    &mut writer,
                    &Message::EditorResized { instance: *instance, width: size.width, height: size.height },
                )?;
            }
        }
        let message = match next_message(&messages) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if message == Message::Quit {
            break;
        }
        if let Some(reply) = answer(&mut instances, message) {
            send(&mut writer, &reply)?;
        }
    }
    for (_, loaded) in std::mem::take(&mut instances) {
        loaded.unload();
    }
    Ok(())
}

/// The reply to one message from the engine (anything but `quit`); `None` for a message that
/// takes no reply (`place_editor`).
fn answer(instances: &mut Instances, message: Message) -> Option<Message> {
    let failed = |instance: u32, message: String| Message::Error { instance: Some(instance), message };
    let missing = |instance: u32| Message::Error { instance: Some(instance), message: "no such instance".into() };
    Some(match message {
        Message::Hello { .. } => Message::Hello { version: VERSION },
        Message::Ping { id } => Message::Pong { id },
        Message::Load { instance, .. } if instances.contains_key(&instance) => {
            failed(instance, format!("instance {instance} is already loaded"))
        }
        Message::Load { instance, .. } if instances.len() >= MAX_INSTANCES => {
            failed(instance, format!("at most {MAX_INSTANCES} plug-ins run in one process"))
        }
        Message::Load { instance, plugin, link, sample_rate } => match load(&plugin, &link, sample_rate) {
            Ok(fresh) => {
                let latency = fresh.latency();
                instances.insert(instance, fresh);
                Message::Loaded { instance, latency }
            }
            Err(message) => failed(instance, message),
        },
        Message::Unload { instance } => {
            if let Some(loaded) = instances.remove(&instance) {
                loaded.unload();
            }
            Message::Unloaded { instance }
        }
        Message::OpenEditor { instance, owner, embedded } => {
            let mode = match (embedded, owner) {
                (true, Some(parent)) => window::Mode::Embedded { parent },
                (true, None) => return Some(failed(instance, "no window to embed the editor in".into())),
                (false, owner) => {
                    window::set_owner(owner);
                    window::Mode::Floating
                }
            };
            match instances.get_mut(&instance).map(|loaded| loaded.open_editor(mode)) {
                Some(Ok(size)) => Message::EditorOpened { instance, width: size.width, height: size.height },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::PlaceEditor { instance, placement } => {
            if let Some(loaded) = instances.get(&instance) {
                loaded.place_editor(placement);
            }
            return None;
        }
        Message::CloseEditor { instance } => {
            if let Some(loaded) = instances.get_mut(&instance) {
                loaded.close_editor();
            }
            Message::EditorClosed { instance }
        }
        Message::GetParams { instance } => match instances.get_mut(&instance).map(Loaded::params) {
            Some(Ok(params)) => Message::Params { instance, params },
            Some(Err(message)) => failed(instance, message),
            None => missing(instance),
        },
        Message::SetParam { instance, id, value } => {
            match instances.get_mut(&instance).map(|p| p.set_param(id, value)) {
                Some(Ok(param)) => Message::Param { instance, param },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::GetState { instance } => match instances.get_mut(&instance).map(Loaded::state) {
            Some(Ok((data, controller))) => Message::State { instance, data, controller },
            Some(Err(message)) => failed(instance, message),
            None => missing(instance),
        },
        Message::SetState { instance, data, controller } => match instances.get_mut(&instance) {
            Some(plugin) => match plugin.set_state(&data, controller.as_deref()).and_then(|()| plugin.state()) {
                Ok((data, controller)) => Message::State { instance, data, controller },
                Err(message) => failed(instance, message),
            },
            None => missing(instance),
        },
        other => Message::Error { instance: other.instance(), message: format!("unexpected message {other:?}") },
    })
}
