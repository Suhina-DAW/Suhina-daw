//! Running a CLAP plug-in (through `clack-host`).
//!
//! The plug-in instance lives on the host's main thread, which also answers control messages
//! and serves the plug-in's main-thread callbacks; the started audio processor lives on the
//! audio thread, which serves blocks from the link. Audio is one output port with as many
//! channels as the link carries, and one input port like it unless the plug-in has no audio
//! input (an instrument). Notes go to note port 0 as note events; controllers and pitch bend as
//! MIDI events. Parameters are read on the main thread; a value set goes to the audio thread
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
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_host::prelude::*;
use loopwerk_ipc::{Event, EventKind, MAX_EVENTS, Served};
use loopwerk_plugins::ParamInfo;

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
    params: OnceLock<Option<PluginParams>>,
}

/// Parameter values waiting for the next block: id and plain value.
type PendingParams = Arc<Mutex<Vec<(u32, f64)>>>;
/// Most values one block carries; more wait for the next.
const MAX_PARAM_CHANGES: usize = 1024;

impl<'a> SharedHandler<'a> for Shared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.state.set(instance.get_extension());
        let _ = self.latency.set(instance.get_extension());
        let _ = self.audio_ports.set(instance.get_extension());
        let _ = self.params.set(instance.get_extension());
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

pub(crate) struct MainThread;

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
        builder.register::<HostLog>().register::<HostState>().register::<HostLatency>();
    }
}

/// A loaded, active CLAP plug-in and its audio thread.
pub(crate) struct ClapPlugin {
    instance: PluginInstance<Host>,
    audio: Option<thread::JoinHandle<StoppedPluginAudioProcessor<Host>>>,
    stop: Arc<AtomicBool>,
    latency: u32,
    pending: PendingParams,
}

/// The entry `spec` names: a CLAP file, or the built-in test plug-in. Also returns the plug-in
/// id asked for, if any.
fn entry(spec: &str) -> Result<(PluginEntry, Option<String>), String> {
    if spec == "test:clap" {
        let entry = PluginEntry::load_from_clack::<crate::test_clap::Entry>(c"loopwerk-test-clap")
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
pub(crate) fn describe(spec: &str) -> Result<Vec<loopwerk_plugins::scan::PluginInfo>, String> {
    let (entry, _) = entry(spec)?;
    let factory = entry.get_plugin_factory().ok_or("the file has no plug-in factory")?;
    let text = |c: Option<&std::ffi::CStr>| c.map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(factory
        .plugin_descriptors()
        .map(|d| loopwerk_plugins::scan::PluginInfo {
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
            HostInfo::new("Loopwerk", "Loopwerk", "https://github.com/juhamikael/Loopwerk", env!("CARGO_PKG_VERSION"))
                .map_err(|e| e.to_string())?;
        let shared = |_: &()| Shared {
            callback_requested: AtomicBool::new(false),
            state: OnceLock::new(),
            latency: OnceLock::new(),
            audio_ports: OnceLock::new(),
            params: OnceLock::new(),
        };
        let mut instance =
            PluginInstance::<Host>::new(shared, |_| MainThread, &entry, &id, &info).map_err(|e| e.to_string())?;

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
            .spawn(move || serve(stopped, served, has_input, audio_pending, &audio_stop))
            .map_err(|e| e.to_string())?;
        Ok(Self { instance, audio: Some(audio), stop, latency, pending })
    }

    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    /// Runs the plug-in's main-thread callback if it asked for one.
    pub(crate) fn tick(&mut self) {
        if self.instance.access_shared_handler(|s| s.callback_requested.swap(false, Ordering::AcqRel)) {
            self.instance.call_on_main_thread_callback();
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
        let value = value.clamp(info.min_value.min(info.max_value), info.max_value.max(info.min_value));
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
    let mut in_events = EventBuffer::with_capacity(MAX_EVENTS);
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
    let span = (info.max_value - info.min_value).abs();
    ParamInfo {
        id: info.id.get(),
        name: text(info.name),
        group: text(info.module),
        min: info.min_value,
        max: info.max_value,
        default: info.default_value,
        value,
        text: shown,
        steps: if stepped && span.is_finite() { span.round().min(f64::from(u32::MAX)) as u32 } else { 0 },
        automatable: info.flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
        read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
        hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
    }
}

/// One event from the link as a CLAP event: notes as note events on note port 0, controllers
/// and pitch bend as MIDI messages.
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
        EventKind::PitchBend { channel, value } => {
            let bend = ((value + 1.0) / 2.0 * 16_383.0).round().clamp(0.0, 16_383.0) as u16;
            let (low, high) = ((bend & 0x7F) as u8, (bend >> 7) as u8);
            buffer.push(&MidiEvent::new(event.frame, 0, [0xE0 | (channel & 0x0F), low, high]));
        }
    }
}
