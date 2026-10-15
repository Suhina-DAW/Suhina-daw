//! Running a CLAP plug-in (through `clack-host`).
//!
//! The plug-in instance lives on the host's main thread, which also answers control messages
//! and serves the plug-in's main-thread callbacks; the started audio processor lives on the
//! audio thread, which serves blocks from the link. Audio is one output port with as many
//! channels as the link carries, and one input port like it unless the plug-in has no audio
//! input (an instrument). Notes go to note port 0 as note events; controllers and pitch bend as
//! MIDI events. The host serves the plug-in's timers on every system and, on Unix, the
//! descriptors it waits on (`timer-support`, `posix-fd-support`; [`crate::run_loop`]), from its
//! main loop. Parameters are read on the main thread; a value set goes to the audio thread
//! and reaches the plug-in as a parameter event at the start of the next block.
//!
//! A plug-in is named `clap:<file>` (the first plug-in in the file) or `clap:<file>#<id>`, or
//! `test:clap` for the gain plug-in built into the host for tests ([`crate::test_clap`]).

use std::ffi::CString;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clack_extensions::audio_ports::PluginAudioPorts;
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl, PluginPosixFd};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_host::prelude::*;
use suhina_ipc::{Event, EventKind, MAX_EVENTS, Served};
use suhina_plugins::ParamInfo;

/// How long the audio thread waits for a block before checking whether it should stop.
const POLL: Duration = Duration::from_millis(100);
/// Output events the audio thread has room for each block.
const EVENT_CAPACITY: usize = 512;

pub(crate) struct Host;

pub(crate) struct Shared {
    callback_requested: AtomicBool,
    state: OnceLock<Option<PluginState>>,
    latency: OnceLock<Option<PluginLatency>>,
    audio_ports: OnceLock<Option<PluginAudioPorts>>,
    timer: OnceLock<Option<PluginTimer>>,
    #[cfg(unix)]
    posix_fd: OnceLock<Option<PluginPosixFd>>,
    params: OnceLock<Option<PluginParams>>,
    gui: OnceLock<Option<PluginGui>>,
    /// A size the plug-in asked its editor's window to have (from any thread).
    gui_resize: Mutex<Option<GuiSize>>,
    /// The plug-in said its editor is gone.
    gui_closed: AtomicBool,
    /// ... and that it destroyed it itself, so the host must not.
    gui_destroyed: AtomicBool,
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        if let Ok(mut wanted) = self.gui_resize.lock() {
            *wanted = Some(new_size);
        }
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, was_destroyed: bool) {
        self.gui_destroyed.store(was_destroyed, Ordering::Release);
        self.gui_closed.store(true, Ordering::Release);
    }
}

/// An open editor: the window the plug-in's view is in.
struct ClapEditor {
    window: crate::window::Window,
    size: crate::window::Size,
    /// The user may resize the window (a floating one the plug-in allows it for).
    resizable: bool,
    /// A size the editor got since the engine was last told, by the plug-in or the user.
    resized: Option<crate::window::Size>,
}

/// How the editor is embedded on this system.
const GUI: GuiConfiguration<'static> =
    GuiConfiguration { api_type: if cfg!(windows) { GuiApiType::WIN32 } else { GuiApiType::X11 }, is_floating: false };

/// Parameter values waiting for the next block: id and plain value.
type PendingParams = Arc<Mutex<Vec<(u32, f64)>>>;
/// Most values one block carries; more wait for the next.
const MAX_PARAM_CHANGES: usize = 1024;

impl<'a> SharedHandler<'a> for Shared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.state.set(instance.get_extension());
        let _ = self.latency.set(instance.get_extension());
        let _ = self.audio_ports.set(instance.get_extension());
        let _ = self.timer.set(instance.get_extension());
        #[cfg(unix)]
        let _ = self.posix_fd.set(instance.get_extension());
        let _ = self.params.set(instance.get_extension());
        let _ = self.gui.set(instance.get_extension());
    }

    fn request_restart(&self) {}

    fn request_process(&self) {}

    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

impl HostLogImpl for Shared {
    fn log(&self, severity: LogSeverity, message: &str) {
        eprintln!("plug-in {severity:?}: {message}");
    }
}

/// The plug-in's timers and (on Unix) the descriptors it waits on, served from the main loop.
pub(crate) struct MainThread {
    timers: std::cell::RefCell<crate::run_loop::Timers<u32>>,
    next_timer: std::cell::Cell<u32>,
    #[cfg(unix)]
    fds: std::cell::RefCell<Vec<(i32, FdFlags)>>,
}

impl MainThread {
    fn new() -> Self {
        Self {
            timers: std::cell::RefCell::new(crate::run_loop::Timers::new()),
            next_timer: std::cell::Cell::new(0),
            #[cfg(unix)]
            fds: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl HostTimerImpl for MainThread {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        let id = self.next_timer.get();
        self.next_timer.set(id.wrapping_add(1));
        let added = self
            .timers
            .try_borrow_mut()
            .is_ok_and(|mut t| t.add(id, std::time::Duration::from_millis(u64::from(period_ms))));
        if added { Ok(TimerId(id)) } else { Err(HostError::Message("too many timers")) }
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        let mut timers = self.timers.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        timers.remove(|id| *id == timer_id.0);
        Ok(())
    }
}

#[cfg(unix)]
impl HostPosixFdImpl for MainThread {
    fn register_fd(&self, fd: i32, flags: FdFlags) -> Result<(), HostError> {
        let mut fds = self.fds.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        if fds.len() >= 64 {
            return Err(HostError::Message("too many descriptors"));
        }
        fds.retain(|(kept, _)| *kept != fd);
        fds.push((fd, flags));
        Ok(())
    }

    fn modify_fd(&self, fd: i32, flags: FdFlags) -> Result<(), HostError> {
        let mut fds = self.fds.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        let slot = fds.iter_mut().find(|(kept, _)| *kept == fd).ok_or(HostError::Message("not registered"))?;
        slot.1 = flags;
        Ok(())
    }

    fn unregister_fd(&self, fd: i32) -> Result<(), HostError> {
        let mut fds = self.fds.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        fds.retain(|(kept, _)| *kept != fd);
        Ok(())
    }
}

impl MainThreadHandler<'_> for MainThread {}

impl HostStateImpl for MainThread {
    fn mark_dirty(&self) {}
}

impl HostLatencyImpl for MainThread {
    fn changed(&self) {}
}

impl HostHandlers for Host {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostState>()
            .register::<HostLatency>()
            .register::<HostGui>()
            .register::<HostTimer>();
        #[cfg(unix)]
        builder.register::<HostPosixFd>();
    }
}

/// A loaded, active CLAP plug-in and its audio thread.
pub(crate) struct ClapPlugin {
    instance: PluginInstance<Host>,
    audio: Option<thread::JoinHandle<StoppedPluginAudioProcessor<Host>>>,
    stop: Arc<AtomicBool>,
    latency: u32,
    pending: PendingParams,
    editor: Option<ClapEditor>,
    /// The plug-in's name, for its editor's window.
    name: String,
}

/// The entry `spec` names: a CLAP file, or the built-in test plug-in. Also returns the plug-in
/// id asked for, if any.
fn entry(spec: &str) -> Result<(PluginEntry, Option<String>), String> {
    if spec == "test:clap" {
        let entry = PluginEntry::load_from_clack::<crate::test_clap::Entry>(c"suhina-test-clap")
            .map_err(|e| format!("test plug-in: {e}"))?;
        return Ok((entry, None));
    }
    let file = spec.strip_prefix("clap:").ok_or_else(|| format!("not a CLAP plug-in: {spec:?}"))?;
    let (path, id) = match file.rsplit_once('#') {
        Some((path, id)) if !id.is_empty() => (path, Some(id.to_string())),
        _ => (file, None),
    };
    // SAFETY: loading a CLAP file runs its initialisation code, which Rust cannot check. That is
    // the job of this process: it exists so that whatever the plug-in does, it does it here and
    // not in the engine (ADR 0006). The entry keeps the library loaded while any instance lives.
    let entry = unsafe { PluginEntry::load(path) }.map_err(|e| format!("could not load the plug-in file: {e}"))?;
    Ok((entry, id))
}

/// The plug-ins the file `spec` names holds, without instantiating any (for scanning).
pub(crate) fn describe(spec: &str) -> Result<Vec<suhina_plugins::scan::PluginInfo>, String> {
    let (entry, _) = entry(spec)?;
    let factory = entry.get_plugin_factory().ok_or("the file has no plug-in factory")?;
    let text = |c: Option<&std::ffi::CStr>| c.map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(factory
        .plugin_descriptors()
        .map(|d| suhina_plugins::scan::PluginInfo {
            path: std::path::PathBuf::new(),
            id: text(d.id()),
            name: text(d.name()),
            vendor: text(d.vendor()),
            version: text(d.version()),
            features: d.features().map(|f| f.to_string_lossy().into_owned()).collect(),
            replaces: Vec::new(),
        })
        .collect())
}

impl ClapPlugin {
    /// Loads the plug-in `spec` names, activates it for the link's block size at `sample_rate`
    /// and starts serving blocks from `served` on an audio thread.
    pub(crate) fn load(spec: &str, served: Served, sample_rate: f64) -> Result<Self, String> {
        let (entry, wanted) = entry(spec)?;
        let factory = entry.get_plugin_factory().ok_or("the file has no plug-in factory")?;
        let ids: Vec<CString> = factory.plugin_descriptors().filter_map(|d| d.id().map(CString::from)).collect();
        let id = match &wanted {
            Some(wanted) => ids.into_iter().find(|id| id.to_bytes() == wanted.as_bytes()),
            None => ids.into_iter().next(),
        }
        .ok_or_else(|| format!("no plug-in {} in {spec:?}", wanted.as_deref().unwrap_or("at all")))?;

        let info =
            HostInfo::new("Suhina", "Suhina", "https://github.com/suhina-daw/suhina-daw", env!("CARGO_PKG_VERSION"))
                .map_err(|e| e.to_string())?;
        let shared = |_: &()| Shared {
            callback_requested: AtomicBool::new(false),
            state: OnceLock::new(),
            latency: OnceLock::new(),
            audio_ports: OnceLock::new(),
            timer: OnceLock::new(),
            #[cfg(unix)]
            posix_fd: OnceLock::new(),
            params: OnceLock::new(),
            gui: OnceLock::new(),
            gui_resize: Mutex::new(None),
            gui_closed: AtomicBool::new(false),
            gui_destroyed: AtomicBool::new(false),
        };
        let mut instance = PluginInstance::<Host>::new(shared, |_| MainThread::new(), &entry, &id, &info)
            .map_err(|e| e.to_string())?;

        let max_frames = served.max_frames();
        let configuration = PluginAudioConfiguration { sample_rate, min_frames_count: 1, max_frames_count: max_frames };
        let stopped = instance.activate(|_, _| (), configuration).map_err(|e| e.to_string())?;
        let latency = match instance.access_shared_handler(|s| s.latency.get().copied().flatten()) {
            Some(latency) => latency.get(&instance.plugin_handle()),
            None => 0,
        };

        // A plug-in without the audio-ports extension gets the default stereo in and out.
        let has_input = match instance.access_shared_handler(|s| s.audio_ports.get().copied().flatten()) {
            Some(ports) => ports.count(&instance.plugin_handle(), true) > 0,
            None => true,
        };

        let stop = Arc::new(AtomicBool::new(false));
        let audio_stop = stop.clone();
        let pending = PendingParams::default();
        let audio_pending = pending.clone();
        let audio = thread::Builder::new()
            .name("plug-in audio".into())
            .spawn(move || {
                let _realtime = crate::real_time_thread();
                serve(stopped, served, has_input, audio_pending, &audio_stop)
            })
            .map_err(|e| e.to_string())?;
        let name = String::from_utf8_lossy(id.to_bytes()).into_owned();
        Ok(Self { instance, audio: Some(audio), stop, latency, pending, editor: None, name })
    }

    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    /// Asks the audio thread to stop and waits up to `wait` for it; whether it stopped.
    pub(crate) fn stop_audio(&mut self, wait: Duration) -> bool {
        crate::audio_stopped(&self.stop, self.audio.as_ref(), wait)
    }

    /// Runs the plug-in's main-thread callback if it asked for one.
    pub(crate) fn tick(&mut self) {
        if self.instance.access_shared_handler(|s| s.callback_requested.swap(false, Ordering::AcqRel)) {
            self.instance.call_on_main_thread_callback();
        }
        self.run_loop();
    }

    /// Calls the plug-in back for the timers that are due and (on Unix) the descriptors that
    /// are ready. The lists are copied first: a callback may register or unregister.
    fn run_loop(&mut self) {
        let due = self.instance.access_handler(|m| {
            m.timers.try_borrow_mut().map(|mut t| t.due(std::time::Instant::now())).unwrap_or_default()
        });
        if let Some(timer) = self.instance.access_shared_handler(|s| s.timer.get().copied().flatten()) {
            let handle = self.instance.plugin_handle();
            for id in due {
                timer.on_timer(&handle, TimerId(id));
            }
        }
        #[cfg(unix)]
        {
            let fds = self.instance.access_handler(|m| m.fds.try_borrow().map(|f| f.clone()).unwrap_or_default());
            let wanted: Vec<i32> = fds.iter().map(|(fd, _)| *fd).collect();
            if let Some(posix_fd) = self.instance.access_shared_handler(|s| s.posix_fd.get().copied().flatten()) {
                let handle = self.instance.plugin_handle();
                for fd in crate::run_loop::ready(&wanted) {
                    posix_fd.on_fd(&handle, fd, FdFlags::READ);
                }
            }
        }
    }

    pub(crate) fn state(&mut self) -> Result<Vec<u8>, String> {
        let state = self.state_extension()?;
        let mut data = Vec::new();
        state.save(&self.instance.plugin_handle(), &mut data).map_err(|e| e.to_string())?;
        Ok(data)
    }

    pub(crate) fn set_state(&mut self, data: &[u8]) -> Result<(), String> {
        let state = self.state_extension()?;
        state.load(&self.instance.plugin_handle(), &mut std::io::Cursor::new(data)).map_err(|e| e.to_string())
    }

    fn gui_extension(&self) -> Option<PluginGui> {
        self.instance.access_shared_handler(|s| s.gui.get().copied().flatten())
    }

    /// Opens the plug-in's editor as `mode` says (or returns the size of the one open).
    pub(crate) fn open_editor(&mut self, mode: crate::window::Mode) -> Result<crate::window::Size, String> {
        if let Some(editor) = &self.editor {
            return Ok(editor.size);
        }
        let gui = self.gui_extension().ok_or("the plug-in has no editor")?;
        self.instance.access_shared_handler(|s| {
            s.gui_closed.store(false, Ordering::Release);
            s.gui_destroyed.store(false, Ordering::Release);
        });
        let handle = self.instance.plugin_handle();
        if !gui.is_api_supported(&handle, GUI) {
            return Err("the plug-in's editor does not run on this system".into());
        }
        gui.create(&handle, GUI).map_err(|_| "the plug-in's editor did not open")?;
        let size = gui
            .get_size(&handle)
            .map(|s| crate::window::Size { width: s.width.clamp(1, 16_384), height: s.height.clamp(1, 16_384) })
            .unwrap_or(crate::window::Size { width: 800, height: 600 });
        let resizable = gui.can_resize(&handle) && mode == crate::window::Mode::Floating;
        let window = match crate::window::Window::open_as(mode, &self.name, size, resizable) {
            Ok(window) => window,
            Err(e) => {
                gui.destroy(&handle);
                return Err(e);
            }
        };
        let parent = parent_window(&window);
        // SAFETY: as above.
        if unsafe { gui.set_parent(&handle, parent) }.is_err() {
            gui.destroy(&handle);
            return Err("the plug-in's editor did not open".into());
        }
        let _ = gui.show(&handle);
        self.editor = Some(ClapEditor { window, size, resizable, resized: None });
        Ok(size)
    }

    /// Puts an embedded editor where the engine says.
    pub(crate) fn place_editor(&self, placement: crate::window::Placement) {
        if let Some(editor) = &self.editor {
            crate::window::place(editor.window.id(), placement);
        }
    }

    /// A size the editor got since this was last asked.
    pub(crate) fn editor_resized(&mut self) -> Option<crate::window::Size> {
        self.editor.as_mut().and_then(|e| e.resized.take())
    }

    pub(crate) fn close_editor(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        let destroyed = self.instance.access_shared_handler(|s| s.gui_destroyed.swap(false, Ordering::AcqRel));
        if let Some(gui) = self.gui_extension().filter(|_| !destroyed) {
            let handle = self.instance.plugin_handle();
            let _ = gui.hide(&handle);
            gui.destroy(&handle);
        }
        drop(editor.window);
    }

    /// Keeps the editor in step with its window; returns whether it was closed.
    pub(crate) fn editor_tick(&mut self) -> bool {
        if self.editor.is_none() {
            return false;
        }
        let gui = self.gui_extension();
        let wanted = self.instance.access_shared_handler(|s| s.gui_resize.lock().ok().and_then(|mut w| w.take()));
        let handle = self.instance.plugin_handle();
        let Some(editor) = &mut self.editor else { return false };
        if let Some(size) = wanted {
            let size = crate::window::Size { width: size.width.clamp(1, 16_384), height: size.height.clamp(1, 16_384) };
            crate::window::resize(editor.window.id(), size);
            editor.size = size;
            editor.resized = Some(size);
        } else if editor.resizable {
            let now = crate::window::client_size(editor.window.id());
            if now != editor.size && now.width > 0 && now.height > 0 {
                if let Some(gui) = gui {
                    let asked = GuiSize { width: now.width, height: now.height };
                    let fitted = gui.adjust_size(&handle, asked).unwrap_or(asked);
                    let _ = gui.set_size(&handle, fitted);
                }
                editor.size = now;
                editor.resized = Some(now);
            }
        }
        let gone = self.instance.access_shared_handler(|s| s.gui_closed.swap(false, Ordering::AcqRel));
        let asked = self.editor.as_ref().is_some_and(|e| crate::window::take_close_request(e.window.id()));
        let closed = asked || gone;
        if closed {
            self.close_editor();
        }
        closed
    }

    fn params_extension(&self) -> Option<PluginParams> {
        self.instance.access_shared_handler(|s| s.params.get().copied().flatten())
    }

    /// The plug-in's parameters, values in their plain ranges.
    pub(crate) fn params(&mut self) -> Vec<ParamInfo> {
        let Some(params) = self.params_extension() else { return Vec::new() };
        let handle = self.instance.plugin_handle();
        let count = params.count(&handle).min(1 << 16);
        (0..count)
            .filter_map(|index| {
                let mut buffer = ParamInfoBuffer::new();
                let info = params.get_info(&handle, index, &mut buffer)?;
                let value = params.get_value(&handle, info.id).unwrap_or(info.default_value);
                Some(describe_param(&params, &handle, &info, value))
            })
            .collect()
    }

    /// Sets parameter `id` to `value` (kept within its range); the plug-in gets it at the start
    /// of the next block.
    pub(crate) fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        let params = self.params_extension().ok_or("the plug-in has no parameters")?;
        let handle = self.instance.plugin_handle();
        let count = params.count(&handle).min(1 << 16);
        let mut buffer = ParamInfoBuffer::new();
        let info = (0..count)
            .find_map(|index| {
                let mut probe = ParamInfoBuffer::new();
                let found = params.get_info(&handle, index, &mut probe)?.id.get() == id;
                found.then_some(index)
            })
            .and_then(|index| params.get_info(&handle, index, &mut buffer))
            .ok_or_else(|| format!("no parameter {id}"))?;
        let (low, high) = (crate::vst3::finite(info.min_value, 0.0), crate::vst3::finite(info.max_value, 1.0));
        let value = value.clamp(low.min(high), high.max(low));
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|(other, _)| *other != id);
            pending.push((id, value));
        }
        Ok(describe_param(&params, &handle, &info, value))
    }

    fn state_extension(&self) -> Result<PluginState, String> {
        self.instance
            .access_shared_handler(|s| s.state.get().copied().flatten())
            .ok_or_else(|| "the plug-in keeps no state".to_string())
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        self.close_editor();
        self.stop.store(true, Ordering::Release);
        if let Some(audio) = self.audio.take()
            && let Ok(stopped) = audio.join()
        {
            self.instance.deactivate(stopped);
        }
    }
}

/// The audio thread: serves blocks from the link through the plug-in until `stop` is set, and
/// hands the stopped processor back for deactivation on the main thread.
fn serve(
    stopped: StoppedPluginAudioProcessor<Host>,
    mut served: Served,
    has_input: bool,
    pending: PendingParams,
    stop: &AtomicBool,
) -> StoppedPluginAudioProcessor<Host> {
    let mut processor = match stopped.start_processing() {
        Ok(started) => started,
        Err(e) => return e.into_stopped_processor(),
    };
    let channels = served.channels() as usize;
    let max_frames = served.max_frames() as usize;
    let mut inputs = vec![vec![0.0f32; max_frames]; channels];
    let mut outputs = vec![vec![0.0f32; max_frames]; channels];
    let mut input_ports = AudioPorts::with_capacity(channels, 1);
    let mut output_ports = AudioPorts::with_capacity(channels, 1);
    // Room for a block's parameter values and its events, so pushing never grows it.
    let mut in_events = EventBuffer::with_capacity(MAX_EVENTS + MAX_PARAM_CHANGES);
    let mut out_events = EventBuffer::with_capacity(EVENT_CAPACITY);
    let mut steady_time = 0u64;

    while !stop.load(Ordering::Acquire) {
        served.serve_with_events(POLL, |input, events, output| {
            let frames = input.len() / channels.max(1);
            for (channel, buffer) in inputs.iter_mut().enumerate() {
                for (frame, sample) in buffer.iter_mut().take(frames).enumerate() {
                    *sample = input.get(frame * channels + channel).copied().unwrap_or(0.0);
                }
            }
            let input_port = has_input.then(|| AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    inputs.iter_mut().map(|b| InputChannel::variable(b.get_mut(..frames).unwrap_or_default())),
                ),
            });
            let audio_in = input_ports.with_input_buffers(input_port);
            let mut audio_out = output_ports.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    outputs.iter_mut().map(|b| b.get_mut(..frames).unwrap_or_default()),
                ),
            }]);
            out_events.clear();
            in_events.clear();
            // Parameter values first, at the block's start; then the notes, in time order.
            if let Ok(mut values) = pending.try_lock() {
                let room = values.len().min(MAX_PARAM_CHANGES);
                for (id, value) in values.drain(..room) {
                    if let Some(id) = ClapId::from_raw(id) {
                        in_events.push(&ParamValueEvent::new(0, id, Pckn::match_all(), value));
                    }
                }
            }
            for event in events {
                push_event(&mut in_events, event);
            }
            let processed = processor.process(
                &audio_in,
                &mut audio_out,
                &InputEvents::from_buffer(&in_events),
                &mut OutputEvents::from_buffer(&mut out_events),
                Some(steady_time),
                None,
            );
            steady_time = steady_time.wrapping_add(frames as u64);
            // A failed block stays silent (the output starts zeroed).
            if processed.is_ok() {
                for (channel, buffer) in outputs.iter().enumerate() {
                    for (frame, sample) in buffer.iter().take(frames).enumerate() {
                        if let Some(out) = output.get_mut(frame * channels + channel) {
                            *out = *sample;
                        }
                    }
                }
            }
        });
    }
    processor.stop_processing()
}

/// The host's window as the parent CLAP's `gui` extension takes on this system.
#[cfg(windows)]
fn parent_window(window: &crate::window::Window) -> clack_extensions::gui::Window<'static, 'static> {
    // SAFETY: the window stays open until the editor is destroyed in `close_editor`.
    unsafe { clack_extensions::gui::Window::from_win32_hwnd(window.handle()) }
}

/// The host's window as the parent CLAP's `gui` extension takes on this system: the X11 id.
#[cfg(not(windows))]
fn parent_window(window: &crate::window::Window) -> clack_extensions::gui::Window<'static, 'static> {
    clack_extensions::gui::Window::from_x11_handle(window.handle() as usize as std::ffi::c_ulong)
}

/// A parameter as the protocol describes it, at `value`.
fn describe_param(
    params: &PluginParams,
    handle: &PluginMainThreadHandle,
    info: &clack_extensions::params::ParamInfo,
    value: f64,
) -> ParamInfo {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes.split(|b| *b == 0).next().unwrap_or_default()).into_owned();
    let mut shown = [0u8; 256];
    let shown = params.value_to_text(handle, info.id, value, &mut shown).map(|t| text(t)).unwrap_or_default();
    let stepped = info.flags.contains(ParamInfoFlags::IS_STEPPED);
    // A plug-in's numbers are not trusted: every one sent on is finite.
    let (min, max) = (crate::vst3::finite(info.min_value, 0.0), crate::vst3::finite(info.max_value, 1.0));
    let span = (max - min).abs();
    ParamInfo {
        id: info.id.get(),
        name: text(info.name),
        group: text(info.module),
        min,
        max,
        default: crate::vst3::finite(info.default_value, min),
        value: crate::vst3::finite(value, min),
        text: shown,
        steps: if stepped && span.is_finite() { span.round().min(f64::from(u32::MAX)) as u32 } else { 0 },
        automatable: info.flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
        read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
        hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
    }
}

/// One event from the link as a CLAP event: notes as note events on note port 0, controllers
/// and pitch bend as MIDI messages, parameter values as parameter events.
fn push_event(buffer: &mut EventBuffer, event: &Event) {
    let note_id = |note: i32| u32::try_from(note).map_or(Match::All, Match::Specific);
    match event.kind {
        EventKind::NoteOn { channel, key, velocity } => {
            let pckn = Pckn::new(0u16, u16::from(channel), u16::from(key), note_id(event.note));
            buffer.push(&NoteOnEvent::new(event.frame, pckn, f64::from(velocity)));
        }
        EventKind::NoteOff { channel, key, velocity } => {
            let pckn = Pckn::new(0u16, u16::from(channel), u16::from(key), note_id(event.note));
            buffer.push(&NoteOffEvent::new(event.frame, pckn, f64::from(velocity)));
        }
        EventKind::Controller { channel, number, value } => {
            let value = (value * 127.0).round().clamp(0.0, 127.0) as u8;
            buffer.push(&MidiEvent::new(event.frame, 0, [0xB0 | (channel & 0x0F), number & 0x7F, value]));
        }
        EventKind::Param { id, value } => {
            if let Some(id) = ClapId::from_raw(id) {
                buffer.push(&ParamValueEvent::new(event.frame, id, Pckn::match_all(), f64::from(value)));
            }
        }
        EventKind::PitchBend { channel, value } => {
            let bend = ((value + 1.0) / 2.0 * 16_383.0).round().clamp(0.0, 16_383.0) as u16;
            let (low, high) = ((bend & 0x7F) as u8, (bend >> 7) as u8);
            buffer.push(&MidiEvent::new(event.frame, 0, [0xE0 | (channel & 0x0F), low, high]));
        }
    }
}
