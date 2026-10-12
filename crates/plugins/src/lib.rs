//! The engine's side of plug-in hosting (ADR 0006): starting `loopwerk-plugin-host` processes,
//! loading plug-in instances into them, talking to them, noticing when one crashes or hangs, and
//! restarting it with its last state.
//!
//! [`Plugin::start`] returns two halves. [`PluginAudio`] goes to the audio thread and runs
//! blocks through the plug-in with a deadline ([`loopwerk_ipc`]); [`Plugin`] stays on a control
//! thread for everything else: health, ping, state, restart. They share only a counter of
//! blocks in a row the host has not answered.
//!
//! One host process can run several instances ([`Launch::group`]): instances started with the
//! same host executable and the same group share the process that a registry in this crate
//! keeps for the group, each with its own audio link. Their requests to the process are taken
//! in turn. When a restart replaces the group's process, the group's generation goes up; the
//! instances loaded into the earlier process then report [`Health::Crashed`], and their own
//! restart loads them into the new process.

pub mod library;
pub mod matching;
pub mod offline;
pub mod protocol;
pub mod scan;
pub mod scan_pool;
pub mod vst2;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use interprocess::TryClone;
use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Name, NameType, Stream, ToFsName,
    ToNsName,
};
pub use loopwerk_ipc::{Event, EventKind, MAX_EVENTS, Outcome};
use loopwerk_ipc::{IpcError, Link};

pub use protocol::ParamInfo;
use protocol::{MAX_LINE, Message, VERSION};

/// Blocks in a row without an answer after which a running host counts as hung.
pub const HUNG_AFTER: u32 = 32;

/// Editor closings remembered for instances that have not asked yet.
const MAX_CLOSED_EDITORS: usize = 4096;

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
    /// Instances started with the same `host` and the same group share one host process
    /// (ADR 0006: by default one per plug-in file; see [`library::group_for`]). `None` gives
    /// the instance a process of its own.
    pub group: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Running,
    /// The host process has exited, or a restart of another instance in it replaced it.
    Crashed,
    /// The host process runs but has not answered [`HUNG_AFTER`] blocks of this instance in a
    /// row.
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
        self.process_with_events(input, &[], output, timeout)
    }

    /// [`PluginAudio::process`] with notes and controllers at frames of the block, for an
    /// instrument; see [`Link::process_with_events`]. Real-time safe.
    pub fn process_with_events(
        &mut self,
        input: &[f32],
        events: &[Event],
        output: &mut [f32],
        timeout: Duration,
    ) -> Outcome {
        let outcome = self.link.process_with_events(input, events, output, timeout);
        if outcome == Outcome::Done {
            self.unanswered.store(0, Ordering::Relaxed);
        } else {
            self.unanswered.fetch_add(1, Ordering::Relaxed);
        }
        outcome
    }
}

/// A plug-in's saved state: the component's (CLAP and VST2 have only this), and a VST3
/// plug-in's controller state when it keeps one apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedState {
    pub data: Vec<u8>,
    pub controller: Option<Vec<u8>>,
}

impl From<Vec<u8>> for SavedState {
    fn from(data: Vec<u8>) -> Self {
        Self { data, controller: None }
    }
}

/// Locks `mutex`, taking it over from a thread that panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `mutex` if nobody holds it now.
fn try_lock<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// A running host process and its control connection.
struct HostProcess {
    child: Child,
    control: Stream,
    replies: Receiver<Message>,
}

impl HostProcess {
    /// Starts `launch.host` and says hello.
    fn start(launch: &Launch) -> Result<Self, PluginError> {
        let (address, name) = control_name()?;
        let listener = ListenerOptions::new().name(name).nonblocking(ListenerNonblockingMode::Accept).create_sync()?;
        let mut child = host_command(&launch.host)
            .args(["--control", &address])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + launch.timeout;
        let control = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e.into());
                }
            }
            if child.try_wait()?.is_some() {
                return Err(PluginError::Protocol("the host exited while starting".into()));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PluginError::Timeout("start"));
            }
            thread::sleep(Duration::from_millis(1));
        };
        control.set_nonblocking(false)?;
        let replies = read_lines(control.try_clone()?);
        Ok(Self { child, control, replies })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, message: &Message) -> Result<(), PluginError> {
        let mut line = message.encode();
        line.push('\n');
        self.control.write_all(line.as_bytes())?;
        self.control.flush()?;
        Ok(())
    }

    /// Ends the process, whatever its state.
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Asks the process to quit, gives it a moment, then ends it.
    fn quit(mut self) {
        let _ = self.send(&Message::Quit);
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        self.kill();
    }
}

/// The host process a group of instances share (or one instance alone), and which of the
/// processes started for the group it is.
struct Group {
    process: Option<HostProcess>,
    /// Goes up with every process started for the group: an instance loaded into an earlier
    /// one has lost its process.
    generation: u64,
    /// Instances whose editor the host said was closed, not yet taken in by them.
    closed_editors: Vec<u32>,
    next_ping: u64,
}

impl Group {
    fn new() -> Self {
        Self { process: None, generation: 0, closed_editors: Vec::new(), next_ping: 0 }
    }

    fn alive(&mut self) -> bool {
        self.process.as_mut().is_some_and(HostProcess::alive)
    }

    /// Ends the current process, if any, whatever its state.
    fn end(&mut self) {
        if let Some(process) = self.process.take() {
            process.kill();
        }
        self.closed_editors.clear();
    }

    /// Ends the current process and starts a new one for the group.
    fn spawn(&mut self, launch: &Launch) -> Result<(), PluginError> {
        self.end();
        self.generation += 1;
        self.process = Some(HostProcess::start(launch)?);
        let hello = self.request(None, &Message::Hello { version: VERSION }, "hello", launch.timeout, |m| {
            matches!(m, Message::Hello { .. })
        });
        match hello {
            Ok(Message::Hello { version }) if version == VERSION => Ok(()),
            Ok(other) => {
                self.end();
                Err(PluginError::Protocol(format!("host speaks another version: {other:?}")))
            }
            Err(e) => {
                self.end();
                Err(e)
            }
        }
    }

    /// Takes in what a message the host sent unasked says.
    fn note(&mut self, message: &Message) {
        if let Message::EditorClosed { instance } = message
            && !self.closed_editors.contains(instance)
            && self.closed_editors.len() < MAX_CLOSED_EDITORS
        {
            self.closed_editors.push(*instance);
        }
    }

    /// Takes in the messages waiting, without waiting for more.
    fn drain(&mut self) {
        let mut waiting = Vec::new();
        if let Some(process) = &self.process {
            while let Ok(message) = process.replies.try_recv() {
                waiting.push(message);
            }
        }
        for message in &waiting {
            self.note(message);
        }
    }

    /// Whether the host said `instance`'s editor was closed (and forgets it).
    fn take_closed(&mut self, instance: u32) -> bool {
        let before = self.closed_editors.len();
        self.closed_editors.retain(|i| *i != instance);
        self.closed_editors.len() != before
    }

    /// Sends `message` (about `instance`, or the process) and waits for the reply `wanted`
    /// accepts, skipping stale ones and noting unasked ones; an `error` about the same
    /// instance (or the process) fails.
    fn request(
        &mut self,
        instance: Option<u32>,
        message: &Message,
        what: &'static str,
        timeout: Duration,
        wanted: impl Fn(&Message) -> bool,
    ) -> Result<Message, PluginError> {
        let process = self.process.as_mut().ok_or_else(|| PluginError::Protocol("the host is not running".into()))?;
        process.send(message)?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let received = match &self.process {
                Some(process) => process.replies.recv_timeout(left),
                None => Err(RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Message::Error { instance: about, message }) if about.is_none() || about == instance => {
                    return Err(PluginError::Host(message));
                }
                Ok(message) if wanted(&message) => return Ok(message),
                Ok(other) => self.note(&other),
                Err(RecvTimeoutError::Timeout) => return Err(PluginError::Timeout(what)),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(PluginError::Protocol("the host closed the control connection".into()));
                }
            }
        }
    }

    /// Whether the process's control thread answers within `timeout`.
    fn ping(&mut self, timeout: Duration) -> bool {
        self.next_ping += 1;
        let id = self.next_ping;
        self.request(None, &Message::Ping { id }, "ping", timeout, |m| *m == Message::Pong { id }).is_ok()
    }

    /// Loads `instance` as `launch` says into the group's process, starting one if none runs.
    /// Returns the generation it was loaded into, its latency and its link.
    fn load(&mut self, launch: &Launch, instance: u32) -> Result<(u64, u32, Link), PluginError> {
        let link = Link::create(launch.channels, launch.max_frames)?;
        if !self.alive() {
            self.spawn(launch)?;
        }
        let load = Message::Load {
            instance,
            plugin: launch.plugin.clone(),
            link: link.address(),
            sample_rate: launch.sample_rate,
        };
        let loaded = self.request(
            Some(instance),
            &load,
            "load",
            launch.timeout,
            |m| matches!(m, Message::Loaded { instance: i, .. } if *i == instance),
        )?;
        let latency = match loaded {
            Message::Loaded { latency, .. } => latency,
            _ => 0,
        };
        Ok((self.generation, latency, link))
    }
}

impl Drop for Group {
    /// The last instance of the group is gone: the process quits.
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            process.quit();
        }
    }
}

/// A group's key: the host executable and the group's name.
type GroupKey = (PathBuf, String);

/// The groups whose process is shared, while any instance in them lives.
static GROUPS: Mutex<BTreeMap<GroupKey, Weak<Mutex<Group>>>> = Mutex::new(BTreeMap::new());

/// The group `launch` asks for: the shared one for its host and group name, or one of its own.
fn group_for_launch(launch: &Launch) -> Arc<Mutex<Group>> {
    let Some(name) = &launch.group else {
        return Arc::new(Mutex::new(Group::new()));
    };
    let mut groups = lock(&GROUPS);
    groups.retain(|_, group| group.strong_count() > 0);
    let key = (launch.host.clone(), name.clone());
    if let Some(group) = groups.get(&key).and_then(Weak::upgrade) {
        return group;
    }
    let group = Arc::new(Mutex::new(Group::new()));
    groups.insert(key, Arc::downgrade(&group));
    group
}

/// A fresh instance number, unique in this process.
fn next_instance() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The control half: one plug-in instance in a host process.
pub struct Plugin {
    launch: Launch,
    group: Arc<Mutex<Group>>,
    instance: u32,
    /// The group's generation this instance was loaded into.
    generation: u64,
    unanswered: Arc<AtomicU32>,
    latency: u32,
    /// The state last read from the plug-in; loaded again after a restart.
    last_state: Option<SavedState>,
    /// Parameters set since the state was last read or loaded, in order; set again after a
    /// restart, on top of that state.
    params_since: Vec<(u32, f64)>,
    /// Whether the plug-in's editor window is open.
    editor_open: bool,
    /// What [`Plugin::health`] last found, for when the process is busy with a sibling.
    last_health: Health,
}

impl Plugin {
    /// Loads the plug-in into a host process and returns both halves: into the running process
    /// of its group ([`Launch::group`]), or a new one when the group has none running or the
    /// instance has no group.
    pub fn start(launch: Launch) -> Result<(Self, PluginAudio), PluginError> {
        let group = group_for_launch(&launch);
        let instance = next_instance();
        let (generation, latency, link) = lock(&group).load(&launch, instance)?;
        let unanswered = Arc::new(AtomicU32::new(0));
        let plugin = Self {
            launch,
            group,
            instance,
            generation,
            unanswered: unanswered.clone(),
            latency,
            last_state: None,
            params_since: Vec::new(),
            editor_open: false,
            last_health: Health::Running,
        };
        Ok((plugin, PluginAudio { link, unanswered }))
    }

    /// Delay the plug-in adds, in frames, as it reported when it loaded.
    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// The host process's id (0 when it has none running), for tests and diagnostics.
    pub fn process_id(&self) -> u32 {
        lock(&self.group).process.as_ref().map_or(0, |p| p.child.id())
    }

    /// How the plug-in is. Never waits: while its process is busy with another request (a
    /// sibling loading, which can take seconds), it says what it found last time, so a window
    /// that asks every frame does not stop.
    pub fn health(&mut self) -> Health {
        let Some(mut group) = try_lock(&self.group) else {
            if self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER {
                return Health::Hung;
            }
            return self.last_health;
        };
        self.last_health = if group.generation != self.generation || !group.alive() {
            Health::Crashed
        } else if self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER {
            Health::Hung
        } else {
            Health::Running
        };
        self.last_health
    }

    /// Whether the host's control thread answers within the launch timeout.
    pub fn ping(&mut self) -> bool {
        let mut group = lock(&self.group);
        group.generation == self.generation && group.ping(self.launch.timeout)
    }

    /// Sends a request about this instance and waits for the reply `wanted` accepts.
    fn request(
        &mut self,
        message: &Message,
        what: &'static str,
        wanted: impl Fn(&Message) -> bool,
    ) -> Result<Message, PluginError> {
        let mut group = lock(&self.group);
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let reply = group.request(Some(self.instance), message, what, self.launch.timeout, wanted);
        if group.take_closed(self.instance) {
            self.editor_open = false;
        }
        reply
    }

    /// Reads the plug-in's state and keeps it for a restart.
    pub fn save_state(&mut self) -> Result<SavedState, PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::GetState { instance },
            "state",
            |m| matches!(m, Message::State { instance: i, .. } if *i == instance),
        )?;
        let Message::State { data, controller, .. } = reply else {
            return Err(PluginError::Protocol("expected state".into()));
        };
        let state = SavedState { data, controller };
        self.last_state = Some(state.clone());
        self.params_since.clear();
        Ok(state)
    }

    /// Hands `state` to the plug-in (and keeps it for a restart). A plug-in that does not take
    /// it answers with an error and keeps the state it had.
    pub fn load_state(&mut self, state: impl Into<SavedState>) -> Result<(), PluginError> {
        let state = state.into();
        let instance = self.instance;
        let message = Message::SetState { instance, data: state.data.clone(), controller: state.controller.clone() };
        self.request(&message, "set_state", |m| matches!(m, Message::State { instance: i, .. } if *i == instance))?;
        self.last_state = Some(state);
        self.params_since.clear();
        Ok(())
    }

    /// The plug-in's parameters with their current values.
    pub fn params(&mut self) -> Result<Vec<ParamInfo>, PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::GetParams { instance },
            "params",
            |m| matches!(m, Message::Params { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::Params { params, .. } => Ok(params),
            _ => Err(PluginError::Protocol("expected parameters".into())),
        }
    }

    /// Sets parameter `id` to `value`, in its own range (see [`ParamInfo`]); the plug-in's
    /// audio follows from its next block. Returns the parameter as it is now. Kept for a
    /// restart.
    pub fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::SetParam { instance, id, value },
            "set_param",
            |m| matches!(m, Message::Param { instance: i, .. } if *i == instance),
        )?;
        let Message::Param { param, .. } = reply else {
            return Err(PluginError::Protocol("expected a parameter".into()));
        };
        self.params_since.retain(|(other, _)| *other != id);
        if self.params_since.len() < protocol::MAX_PARAMS {
            self.params_since.push((id, param.value));
        }
        Ok(param)
    }

    /// Opens the plug-in's editor in a window of its own (in the host process) and returns the
    /// size of its inside. Opening an open editor just returns its size.
    pub fn open_editor(&mut self) -> Result<(u32, u32), PluginError> {
        self.open_editor_owned(None)
    }

    /// [`Plugin::open_editor`] in a window that `owner` (the engine's window: a Win32 `HWND`,
    /// an X11 window id) owns: it floats above it and goes with it.
    pub fn open_editor_owned(&mut self, owner: Option<u64>) -> Result<(u32, u32), PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::OpenEditor { instance, owner },
            "open_editor",
            |m| matches!(m, Message::EditorOpened { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::EditorOpened { width, height, .. } => {
                self.editor_open = true;
                Ok((width, height))
            }
            _ => Err(PluginError::Protocol("expected the editor".into())),
        }
    }

    /// Closes the plug-in's editor, if it is open.
    pub fn close_editor(&mut self) -> Result<(), PluginError> {
        let instance = self.instance;
        self.request(
            &Message::CloseEditor { instance },
            "close_editor",
            |m| matches!(m, Message::EditorClosed { instance: i } if *i == instance),
        )?;
        self.editor_open = false;
        Ok(())
    }

    /// Whether the editor is open: the user may have closed its window since it was opened.
    /// Never waits: while the process is busy with another request, the answer is the last
    /// one known.
    pub fn editor_open(&mut self) -> bool {
        let Some(mut group) = try_lock(&self.group) else { return self.editor_open };
        if group.generation != self.generation {
            self.editor_open = false;
        } else {
            group.drain();
            if group.take_closed(self.instance) {
                self.editor_open = false;
            }
        }
        self.editor_open
    }

    /// Brings the plug-in back with its last saved state and the parameters set since, and
    /// returns the new audio half, which replaces the old one.
    ///
    /// When the process is gone, does not answer, or this instance hung in it (its audio
    /// thread is stuck there, so it cannot be let go of), the process is ended and a new one
    /// started for the group; the other instances in it then report [`Health::Crashed`], and
    /// their own restart loads them into the new process. An instance whose process was
    /// already replaced by another's restart is loaded into the new one. An instance with a
    /// process of its own always gets a new process.
    pub fn restart(&mut self) -> Result<PluginAudio, PluginError> {
        let own_hung = self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER;
        let shared = self.launch.group.is_some();
        let group = self.group.clone();
        let mut locked = lock(&group);
        let current = locked.generation == self.generation;
        let alive = locked.alive();
        if !shared || !alive || (current && (own_hung || !locked.ping(self.launch.timeout))) {
            locked.end();
        } else if current {
            let instance = self.instance;
            let unload = Message::Unload { instance };
            let _ = locked.request(
                Some(instance),
                &unload,
                "unload",
                self.launch.timeout,
                |m| matches!(m, Message::Unloaded { instance: i } if *i == instance),
            );
            locked.take_closed(instance);
        }
        let instance = next_instance();
        let (generation, latency, link) = locked.load(&self.launch, instance)?;
        drop(locked);
        self.instance = instance;
        self.generation = generation;
        self.latency = latency;
        self.editor_open = false;
        self.unanswered = Arc::new(AtomicU32::new(0));
        let audio = PluginAudio { link, unanswered: self.unanswered.clone() };
        let params = std::mem::take(&mut self.params_since);
        if let Some(state) = self.last_state.clone() {
            self.load_state(state)?;
        }
        for (id, value) in params {
            self.set_param(id, value)?;
        }
        Ok(audio)
    }
}

impl Drop for Plugin {
    /// Lets go of the instance; the process quits when its last instance is gone.
    fn drop(&mut self) {
        let mut group = lock(&self.group);
        if self.launch.group.is_none() || group.generation != self.generation || !group.alive() {
            return;
        }
        let instance = self.instance;
        let unloaded = group.request(
            Some(instance),
            &Message::Unload { instance },
            "unload",
            self.launch.timeout,
            |m| matches!(m, Message::Unloaded { instance: i } if *i == instance),
        );
        group.take_closed(instance);
        if matches!(unloaded, Err(PluginError::Timeout(_))) {
            // A process that does not answer is no use to the other instances either.
            group.end();
        }
    }
}

/// A command that runs the plug-in host executable `host` without a window of its own: on
/// Windows a console program started from a program without a console (the desktop app) would
/// otherwise open a console window for every host and scan worker. Its own windows (plug-in
/// editors) still open.
pub fn host_command(host: &std::path::Path) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(host);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW` from the Windows process creation flags.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
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
