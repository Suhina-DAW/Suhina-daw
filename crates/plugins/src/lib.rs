//! The engine's side of plug-in hosting (ADR 0006): starting a `loopwerk-plugin-host` process,
//! talking to it, noticing when it crashes or hangs, and restarting it with its last state.
//!
//! [`Plugin::start`] returns two halves. [`PluginAudio`] goes to the audio thread and runs
//! blocks through the plug-in with a deadline ([`loopwerk_ipc`]); [`Plugin`] stays on a control
//! thread for everything else: health, ping, state, restart. They share only a counter of
//! blocks in a row the host has not answered.

pub mod library;
pub mod protocol;
pub mod scan;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use interprocess::TryClone;
use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Name, NameType, Stream, ToFsName,
    ToNsName,
};
use loopwerk_ipc::{IpcError, Link, Outcome};

use protocol::{MAX_LINE, Message, VERSION};

/// Blocks in a row without an answer after which a running host counts as hung.
pub const HUNG_AFTER: u32 = 32;

#[derive(Debug)]
pub enum PluginError {
    Io(std::io::Error),
    Ipc(IpcError),
    /// The host did not answer in time; the text says to what.
    Timeout(&'static str),
    /// The host broke the protocol or went away.
    Protocol(String),
    /// The host reported an error, such as a plug-in it cannot load.
    Host(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "plug-in host: {e}"),
            Self::Ipc(e) => write!(f, "plug-in host: {e}"),
            Self::Timeout(what) => write!(f, "plug-in host did not answer: {what}"),
            Self::Protocol(why) => write!(f, "plug-in host: {why}"),
            Self::Host(message) => write!(f, "plug-in host: {message}"),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<IpcError> for PluginError {
    fn from(e: IpcError) -> Self {
        Self::Ipc(e)
    }
}

/// How to start a plug-in.
#[derive(Debug, Clone)]
pub struct Launch {
    /// The `loopwerk-plugin-host` executable.
    pub host: PathBuf,
    /// What to load, as the host names it.
    pub plugin: String,
    pub channels: u32,
    pub max_frames: u32,
    pub sample_rate: f64,
    /// How long to wait for the host to start, load the plug-in or answer a request.
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Running,
    /// The host process has exited.
    Crashed,
    /// The host process runs but has not answered [`HUNG_AFTER`] blocks in a row.
    Hung,
}

/// The audio thread's half: runs blocks through the plug-in.
pub struct PluginAudio {
    link: Link,
    unanswered: Arc<AtomicU32>,
}

impl PluginAudio {
    /// One block through the plug-in, waiting at most `timeout`; see [`Link::process`].
    /// Real-time safe.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], timeout: Duration) -> Outcome {
        let outcome = self.link.process(input, output, timeout);
        if outcome == Outcome::Done {
            self.unanswered.store(0, Ordering::Relaxed);
        } else {
            self.unanswered.fetch_add(1, Ordering::Relaxed);
        }
        outcome
    }
}

/// The control half: one host process running one plug-in.
pub struct Plugin {
    launch: Launch,
    child: Child,
    control: Stream,
    replies: Receiver<Message>,
    unanswered: Arc<AtomicU32>,
    latency: u32,
    next_ping: u64,
    /// The state last read from the plug-in; loaded again after a restart.
    last_state: Option<Vec<u8>>,
}

impl Plugin {
    /// Starts a host process, loads the plug-in and returns both halves.
    pub fn start(launch: Launch) -> Result<(Self, PluginAudio), PluginError> {
        let link = Link::create(launch.channels, launch.max_frames)?;
        let (address, name) = control_name()?;
        let listener = ListenerOptions::new().name(name).nonblocking(ListenerNonblockingMode::Accept).create_sync()?;
        let mut child = Command::new(&launch.host)
            .args(["--control", &address])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + launch.timeout;
        let control = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if child.try_wait()?.is_some() {
                return Err(PluginError::Protocol("the host exited while starting".into()));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err(PluginError::Timeout("start"));
            }
            thread::sleep(Duration::from_millis(1));
        };
        control.set_nonblocking(false)?;
        let replies = read_lines(control.try_clone()?);
        let unanswered = Arc::new(AtomicU32::new(0));
        let mut plugin = Self {
            launch,
            child,
            control,
            replies,
            unanswered: unanswered.clone(),
            latency: 0,
            next_ping: 0,
            last_state: None,
        };
        plugin.send(&Message::Hello { version: VERSION })?;
        match plugin.reply("hello", |m| matches!(m, Message::Hello { .. }))? {
            Message::Hello { version } if version == VERSION => {}
            other => return Err(PluginError::Protocol(format!("host speaks another version: {other:?}"))),
        }
        let load = Message::Load {
            plugin: plugin.launch.plugin.clone(),
            link: link.address(),
            sample_rate: plugin.launch.sample_rate,
        };
        plugin.send(&load)?;
        if let Message::Loaded { latency } = plugin.reply("load", |m| matches!(m, Message::Loaded { .. }))? {
            plugin.latency = latency;
        }
        Ok((plugin, PluginAudio { link, unanswered }))
    }

    /// Delay the plug-in adds, in frames, as it reported when it loaded.
    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn health(&mut self) -> Health {
        if !matches!(self.child.try_wait(), Ok(None)) {
            Health::Crashed
        } else if self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER {
            Health::Hung
        } else {
            Health::Running
        }
    }

    /// Whether the host's control thread answers within the launch timeout.
    pub fn ping(&mut self) -> bool {
        self.next_ping += 1;
        let id = self.next_ping;
        self.send(&Message::Ping { id }).is_ok() && self.reply("ping", |m| *m == Message::Pong { id }).is_ok()
    }

    /// Reads the plug-in's state and keeps it for a restart.
    pub fn save_state(&mut self) -> Result<Vec<u8>, PluginError> {
        self.send(&Message::GetState)?;
        let Message::State { data } = self.reply("state", |m| matches!(m, Message::State { .. }))? else {
            return Err(PluginError::Protocol("expected state".into()));
        };
        self.last_state = Some(data.clone());
        Ok(data)
    }

    /// Hands `data` to the plug-in (and keeps it for a restart).
    pub fn load_state(&mut self, data: Vec<u8>) -> Result<(), PluginError> {
        self.send(&Message::SetState { data: data.clone() })?;
        self.reply("set_state", |m| matches!(m, Message::State { .. }))?;
        self.last_state = Some(data);
        Ok(())
    }

    /// Ends the current host process, whatever its state, and starts a new one with the same
    /// plug-in and its last saved state. Returns the new audio half, which replaces the old one.
    pub fn restart(&mut self) -> Result<PluginAudio, PluginError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (mut fresh, audio) = Self::start(self.launch.clone())?;
        if let Some(state) = self.last_state.take() {
            fresh.load_state(state)?;
        }
        *self = fresh;
        Ok(audio)
    }

    fn send(&mut self, message: &Message) -> Result<(), PluginError> {
        let mut line = message.encode();
        line.push('\n');
        self.control.write_all(line.as_bytes())?;
        self.control.flush()?;
        Ok(())
    }

    /// Waits for the reply `wanted` accepts, skipping stale ones; an `error` reply fails.
    fn reply(&mut self, what: &'static str, wanted: impl Fn(&Message) -> bool) -> Result<Message, PluginError> {
        let deadline = Instant::now() + self.launch.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.replies.recv_timeout(left) {
                Ok(Message::Error { message }) => return Err(PluginError::Host(message)),
                Ok(message) if wanted(&message) => return Ok(message),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return Err(PluginError::Timeout(what)),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(PluginError::Protocol("the host closed the control connection".into()));
                }
            }
        }
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.send(&Message::Quit);
        let deadline = Instant::now() + Duration::from_millis(500);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A fresh name for the control socket: the address to pass to the host, and the name itself.
fn control_name() -> Result<(String, Name<'static>), PluginError> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = format!("loopwerk-control-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    parse_address(&if GenericNamespaced::is_supported() {
        format!("ns:{id}")
    } else {
        format!("fs:{}", std::env::temp_dir().join(id).display())
    })
}

/// The control socket an address names: `ns:<name>` (a namespaced socket or pipe) or
/// `fs:<path>` (a socket file, where the system has no namespace).
pub fn parse_address(address: &str) -> Result<(String, Name<'static>), PluginError> {
    let bad = |e: std::io::Error| PluginError::Protocol(format!("bad control address {address:?}: {e}"));
    let name = if let Some(name) = address.strip_prefix("ns:") {
        name.to_string().to_ns_name::<GenericNamespaced>().map_err(bad)?
    } else if let Some(path) = address.strip_prefix("fs:") {
        PathBuf::from(path).to_fs_name::<GenericFilePath>().map_err(bad)?
    } else {
        return Err(PluginError::Protocol(format!("bad control address {address:?}")));
    };
    Ok((address.to_string(), name))
}

/// Reads messages from `stream` on a thread of its own, so replies can be waited for with a
/// timeout. Unknown lines are skipped; the channel closes when the stream ends.
pub fn read_lines(stream: Stream) -> Receiver<Message> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match (&mut reader).take(MAX_LINE as u64 + 1).read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if let Some(message) = Message::decode(line.trim_end())
                        && sender.send(message).is_err()
                    {
                        return;
                    }
                }
            }
        }
    });
    receiver
}

/// Connects to the engine's control socket at `address` (the host side).
pub fn connect(address: &str) -> Result<Stream, PluginError> {
    let (_, name) = parse_address(address)?;
    Ok(Stream::connect(name)?)
}
