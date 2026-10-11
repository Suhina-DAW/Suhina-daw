//! Running a CLAP plug-in (through `clack-host`).
//!
//! The plug-in instance lives on the host's main thread, which also answers control messages
//! and serves the plug-in's main-thread callbacks; the started audio processor lives on the
//! audio thread, which serves blocks from the link. Audio is stereo in and out for now (one
//! port each, as many channels as the link carries); reading the plug-in's audio-port layout
//! comes with instruments and mono effects.
//!
//! A plug-in is named `clap:<file>` (the first plug-in in the file) or `clap:<file>#<id>`, or
//! `test:clap` for the gain plug-in built into the host for tests ([`crate::test_clap`]).

use std::ffi::CString;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_host::prelude::*;
use loopwerk_ipc::Served;

/// How long the audio thread waits for a block before checking whether it should stop.
const POLL: Duration = Duration::from_millis(100);
/// Output events the audio thread has room for each block.
const EVENT_CAPACITY: usize = 512;

pub(crate) struct Host;

pub(crate) struct Shared {
    callback_requested: AtomicBool,
    state: OnceLock<Option<PluginState>>,
    latency: OnceLock<Option<PluginLatency>>,
}

impl<'a> SharedHandler<'a> for Shared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.state.set(instance.get_extension());
        let _ = self.latency.set(instance.get_extension());
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
    let entry = unsafe { PluginEntry::load(path) }.map_err(|e| format!("loading {path:?}: {e}"))?;
    Ok((entry, id))
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

        let stop = Arc::new(AtomicBool::new(false));
        let audio_stop = stop.clone();
        let audio = thread::Builder::new()
            .name("plug-in audio".into())
            .spawn(move || serve(stopped, served, &audio_stop))
            .map_err(|e| e.to_string())?;
        Ok(Self { instance, audio: Some(audio), stop, latency })
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
    let no_events = EventBuffer::new();
    let mut out_events = EventBuffer::with_capacity(EVENT_CAPACITY);
    let mut steady_time = 0u64;

    while !stop.load(Ordering::Acquire) {
        served.serve(POLL, |input, output| {
            let frames = input.len() / channels.max(1);
            for (channel, buffer) in inputs.iter_mut().enumerate() {
                for (frame, sample) in buffer.iter_mut().take(frames).enumerate() {
                    *sample = input.get(frame * channels + channel).copied().unwrap_or(0.0);
                }
            }
            let audio_in = input_ports.with_input_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    inputs.iter_mut().map(|b| InputChannel::variable(b.get_mut(..frames).unwrap_or_default())),
                ),
            }]);
            let mut audio_out = output_ports.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    outputs.iter_mut().map(|b| b.get_mut(..frames).unwrap_or_default()),
                ),
            }]);
            out_events.clear();
            let processed = processor.process(
                &audio_in,
                &mut audio_out,
                &InputEvents::from_buffer(&no_events),
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
