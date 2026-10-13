//! The audio device: lists the system's hosts and output devices and plays an engine
//! [`Renderer`] on the one chosen ([`OutputChoice`]), or on the default output.
//!
//! The renderer is built on the audio owner's thread and then moved to a render thread of its
//! own, which runs it ahead of the device in blocks of the chosen buffer size (see
//! [`engine_block`]) into a lock-free ring ([`rtrb`]); the device's callback only copies from
//! the ring, converts samples and sets atomics: no allocation, locks or I/O on either thread
//! (AGENTS.md rule 3). The plug-ins of a block get a share of that block's playing time
//! (ADR 0006), so the deadline follows the buffer the user chose. The ring keeps two blocks
//! ahead, or a device buffer and a block when the device asks for more at once. Launcher
//! commands reach the renderer through its lock-free command queue, and stretched clips are
//! rendered ahead on a worker thread of their own (ADR 0003).
//!
//! Hosts are cpal's (WASAPI shared on Windows, ASIO with the `asio` feature, ALSA, JACK,
//! CoreAudio), and WASAPI exclusive mode through the `wasapi` crate. A stream is opened and
//! closed on the audio owner's thread, never on the render or device thread.

pub mod devices;
#[cfg(windows)]
mod exclusive;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use loopwerk_engine::model::Project;
use loopwerk_engine::{AudioPool, Controller, Plugins, Renderer};
use loopwerk_session::Player;
use loopwerk_session::audio::{
    AudioSystem, HostInfo, LiveOutput, MAX_BUFFER_FRAMES, MIN_BUFFER_FRAMES, Opened, OutputChoice, OutputInfo,
};

/// Frames the engine renders per block when no buffer size is chosen.
pub const ENGINE_BLOCK: usize = 1024;
/// Blocks the ring keeps ahead of the device: the latency from the engine to the device is up
/// to this many blocks (more when the device takes more than a block at once).
const RING_BLOCKS: usize = 2;
/// The largest device buffer the ring makes room for, in frames.
const MAX_DEVICE_FRAMES: usize = 8192;
/// How long the render thread waits when the ring is full before looking again.
const RENDER_WAIT: Duration = Duration::from_millis(1);
/// How long closing a stream may take before its plug-ins are given up.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether an ASIO stream is open: only one ASIO driver can be loaded, so listing ASIO devices
/// meanwhile shows the ones listed before.
static ASIO_OPEN: AtomicBool = AtomicBool::new(false);

/// The device's end of the ring.
pub(crate) type Ring = rtrb::Consumer<f32>;

/// State the audio callback shares with the main thread.
#[derive(Default)]
pub struct Progress {
    frames: AtomicU64,
    underruns: AtomicU64,
    stream_error: AtomicBool,
    /// Frames the device asked for in its last callback.
    callback_frames: AtomicUsize,
    /// The last measured latency, engine to speaker, in microseconds.
    latency_us: AtomicU64,
}

impl Progress {
    /// Frames the device has been given so far.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Device buffers the render thread had not filled in time, so far: those played silence
    /// for what was missing.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Whether the device reported an error.
    pub fn failed(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
    }

    /// Frames the device asked for in its last callback; 0 before the first.
    pub fn callback_frames(&self) -> usize {
        self.callback_frames.load(Ordering::Relaxed)
    }

    /// The last measured time from the engine rendering a frame to the device playing it.
    pub fn latency(&self) -> Duration {
        Duration::from_micros(self.latency_us.load(Ordering::Relaxed))
    }

    pub(crate) fn fail(&self) {
        self.stream_error.store(true, Ordering::Relaxed);
    }

    pub(crate) fn set_latency(&self, latency: Duration) {
        self.latency_us.store(u64::try_from(latency.as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    pub(crate) fn set_callback_frames(&self, frames: usize) {
        self.callback_frames.store(frames, Ordering::Relaxed);
    }

    pub(crate) fn add_frames(&self, frames: usize) {
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
    }
}

/// Fills `out` (interleaved stereo) from the ring, with silence, counted as an underrun, for
/// what is not there yet. Never allocates.
pub(crate) fn read_ring(ring: &mut Ring, out: &mut [f32], progress: &Progress) {
    let ready = ring.slots().min(out.len()) / 2 * 2;
    match ring.read_chunk(ready) {
        Ok(chunk) => {
            for (sample, slot) in chunk.into_iter().zip(out.iter_mut()) {
                *slot = sample;
            }
        }
        Err(_) => out.fill(0.0),
    }
    if ready < out.len() {
        if let Some(missing) = out.get_mut(ready..) {
            missing.fill(0.0);
        }
        progress.underruns.fetch_add(1, Ordering::Relaxed);
    }
}

/// The time from the engine to the speaker: the frames queued in the ring, played at
/// `sample_rate`, and the device's own delay after the callback.
pub(crate) fn measure_latency(queued_frames: usize, device: Duration, sample_rate: u32) -> Duration {
    let ring = Duration::from_secs_f64(queued_frames as f64 / f64::from(sample_rate.max(1)));
    ring.saturating_add(device)
}

/// The frames the engine renders per block: `LOOPWERK_ENGINE_BLOCK` (64 to 4096) when set, to
/// try a size in tests; else the buffer size chosen, within 64 … 4096; else [`ENGINE_BLOCK`].
pub fn engine_block(chosen: Option<u32>) -> usize {
    let overridden = std::env::var("LOOPWERK_ENGINE_BLOCK").ok();
    block_for(chosen, overridden.as_deref())
}

fn block_for(chosen: Option<u32>, overridden: Option<&str>) -> usize {
    let range = MIN_BUFFER_FRAMES as usize..=MAX_BUFFER_FRAMES as usize;
    overridden
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| range.contains(n))
        .or_else(|| chosen.map(|n| (n as usize).clamp(*range.start(), *range.end())))
        .unwrap_or(ENGINE_BLOCK)
}

/// Frames the render thread keeps ahead in the ring: two blocks, or a device buffer and a
/// block when the device takes more at once.
fn ring_target(block: usize, device_frames: usize) -> usize {
    (RING_BLOCKS * block).max(device_frames.min(MAX_DEVICE_FRAMES) + block)
}

/// The device's side of a playing stream.
enum DeviceStream {
    Cpal(cpal::Stream),
    #[cfg(windows)]
    Exclusive(exclusive::ExclusiveStream),
}

impl DeviceStream {
    fn play(&self) -> Result<(), String> {
        match self {
            Self::Cpal(stream) => stream.play().map_err(|e| e.to_string()),
            #[cfg(windows)]
            Self::Exclusive(stream) => {
                stream.play();
                Ok(())
            }
        }
    }

    /// Frames per device buffer, when the device says before it plays.
    fn buffer_frames(&self) -> Option<u32> {
        match self {
            Self::Cpal(stream) => stream.buffer_size().ok().filter(|n| *n > 0),
            #[cfg(windows)]
            Self::Exclusive(stream) => Some(stream.buffer_frames),
        }
    }
}

/// A playing stream: the device's, and the render thread that feeds it. Dropping it stops
/// both.
pub struct Stream {
    device: Option<DeviceStream>,
    stop: Arc<AtomicBool>,
    render: Option<std::thread::JoinHandle<Renderer>>,
    asio: bool,
}

impl Stream {
    /// Stops the device, then the render thread, and gives back the renderer, so its plug-ins
    /// can move to the next one ([`Renderer::into_plugins`]).
    pub fn close(mut self) -> Option<Renderer> {
        self.stop_all()
    }

    fn stop_all(&mut self) -> Option<Renderer> {
        drop(self.device.take());
        if self.asio {
            ASIO_OPEN.store(false, Ordering::Release);
            self.asio = false;
        }
        self.stop.store(true, Ordering::Release);
        self.render.take().and_then(|render| render.join().ok())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _renderer = self.stop_all();
    }
}

enum Backend {
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        format: cpal::SampleFormat,
    },
    /// WASAPI exclusive mode, on the device with this name (the default when `None`).
    #[cfg(windows)]
    Exclusive {
        device: Option<String>,
        buffer: Option<u32>,
    },
}

/// An output device, the format it is opened with and the engine's block on it.
pub struct Output {
    backend: Backend,
    host: String,
    device_name: String,
    sample_rate: u32,
    channels: u16,
    block: usize,
    exclusive: bool,
    /// What could not be had as chosen.
    notices: Vec<String>,
}

impl Output {
    /// The default output device in the format it asks for.
    pub fn open() -> Result<Self, String> {
        Self::choose(&OutputChoice::default())
    }

    /// The output `choice` names, made to fit what the device offers: a host or device that is
    /// not there, or a sample rate it does not offer, falls back to the default with a notice
    /// ([`Output::notices`]); a buffer size goes to the nearest the driver allows.
    pub fn choose(choice: &OutputChoice) -> Result<Self, String> {
        let mut notices = Vec::new();
        let default_host = cpal::default_host().id();
        let host_id = match &choice.host {
            Some(name) => match cpal::available_hosts().into_iter().find(|h| devices::same_name(h.name(), name)) {
                Some(id) => id,
                None => {
                    notices.push(format!("the audio host {name} is not available; {} plays", default_host.name()));
                    default_host
                }
            },
            None => default_host,
        };
        let host = cpal::host_from_id(host_id).map_err(|e| format!("audio host {}: {e}", host_id.name()))?;
        let host_name = host_id.name().to_string();
        let chosen = choice.device.as_ref().and_then(|name| {
            let found =
                host.output_devices().ok().and_then(|mut list| list.find(|d| devices::same_name(&d.to_string(), name)));
            if found.is_none() {
                notices.push(format!("the audio device {name} is not there; the default output plays"));
            }
            found
        });
        let device = match chosen {
            Some(device) => device,
            None => host.default_output_device().ok_or("no audio output device found")?,
        };
        let device_name = device.to_string();
        let default = device.default_output_config().map_err(|e| format!("audio output {device_name}: {e}"))?;

        #[cfg(windows)]
        if choice.exclusive && host_name == "WASAPI" {
            let sample_rate = choice.sample_rate.unwrap_or(default.sample_rate());
            return Ok(Self {
                backend: Backend::Exclusive { device: Some(device_name.clone()), buffer: choice.buffer_frames },
                host: host_name,
                device_name,
                sample_rate,
                channels: default.channels().max(1),
                block: engine_block(choice.buffer_frames),
                exclusive: true,
                notices,
            });
        }

        let mut supported = default;
        if let Some(rate) = choice.sample_rate.filter(|r| *r != supported.sample_rate()) {
            let channels = supported.channels();
            let format = supported.sample_format();
            let found = device.supported_output_configs().ok().and_then(|configs| {
                let fitting: Vec<_> = configs
                    .filter(|c| c.channels() == channels && (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate))
                    .collect();
                let same_format = fitting.iter().position(|c| c.sample_format() == format);
                let index = same_format.or(if fitting.is_empty() { None } else { Some(0) })?;
                fitting.into_iter().nth(index).and_then(|c| c.try_with_sample_rate(rate))
            });
            match found {
                Some(config) => supported = config,
                None => notices.push(format!(
                    "{device_name} does not play at {rate} Hz; it plays at {} Hz",
                    supported.sample_rate()
                )),
            }
        }
        let range = devices::buffer_range(supported.buffer_size());
        let buffer = choice.buffer_frames.map(|wanted| devices::fit_buffer(wanted, &devices::allowed_buffers(range)));
        let format = supported.sample_format();
        let mut config: cpal::StreamConfig = supported.into();
        if config.channels == 0 {
            return Err(format!("audio output {device_name} reports no channels"));
        }
        if let (Some(frames), Some(_)) = (buffer, range) {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let exclusive = host_name == "ASIO";
        Ok(Self {
            backend: Backend::Cpal { device, config, format },
            host: host_name,
            device_name,
            sample_rate: config.sample_rate,
            channels: config.channels,
            block: engine_block(buffer),
            exclusive,
            notices,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames the engine renders per block on this output.
    pub fn block(&self) -> usize {
        self.block
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// What could not be had as chosen.
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// The device's name, host, sample rate and channels, for messages.
    pub fn describe(&self) -> String {
        let mode = match &self.backend {
            Backend::Cpal { format, .. } => format!("{format}"),
            #[cfg(windows)]
            Backend::Exclusive { .. } => "exclusive".to_string(),
        };
        format!(
            "{} ({}, {} Hz, {} channels, {mode}, {}-frame blocks)",
            self.device_name, self.host, self.sample_rate, self.channels, self.block
        )
    }

    /// Opens the device's stream, reading from `ring`; it plays once [`DeviceStream::play`]
    /// is called.
    fn open_device(&self, ring: Ring, progress: &Arc<Progress>) -> Result<DeviceStream, String> {
        match &self.backend {
            Backend::Cpal { device, config, format } => {
                let (device, config) = (device, *config);
                let stream = match format {
                    cpal::SampleFormat::F32 => build::<f32>(device, config, ring, progress),
                    cpal::SampleFormat::I16 => build::<i16>(device, config, ring, progress),
                    cpal::SampleFormat::I32 => build::<i32>(device, config, ring, progress),
                    cpal::SampleFormat::U16 => build::<u16>(device, config, ring, progress),
                    other => {
                        return Err(format!(
                            "audio output {device} uses sample format {other}, which is not supported yet"
                        ));
                    }
                }?;
                Ok(DeviceStream::Cpal(stream))
            }
            #[cfg(windows)]
            Backend::Exclusive { device, buffer } => {
                exclusive::open(device.clone(), self.sample_rate, *buffer, ring, progress).map(DeviceStream::Exclusive)
            }
        }
    }

    /// Starts playing `renderer` on the device, rendered ahead on a thread of its own, with
    /// its stretched clips rendered ahead on a worker thread that `controller` keeps (and
    /// stops when it is dropped). The stream plays until it is dropped.
    pub fn start(
        &self,
        mut renderer: Renderer,
        controller: &mut Controller,
        progress: &Arc<Progress>,
    ) -> Result<Stream, String> {
        let (producer, consumer) = ring(self.block);
        let device = self.open_device(consumer, progress)?;
        controller.run_stretch_worker(&mut renderer).map_err(|e| format!("could not start the stretch worker: {e}"))?;
        self.play(device, renderer, producer, progress)
    }

    fn play(
        &self,
        device: DeviceStream,
        renderer: Renderer,
        producer: rtrb::Producer<f32>,
        progress: &Arc<Progress>,
    ) -> Result<Stream, String> {
        if let Some(frames) = device.buffer_frames() {
            progress.set_callback_frames(frames as usize);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (stopping, watching, block) = (Arc::clone(&stop), Arc::clone(progress), self.block);
        let render = std::thread::Builder::new()
            .name("loopwerk-render".into())
            .spawn(move || render_ahead(renderer, producer, block, &stopping, &watching))
            .map_err(|e| format!("could not start the render thread: {e}"))?;
        let asio = self.host == "ASIO";
        if asio {
            ASIO_OPEN.store(true, Ordering::Release);
        }
        let stream = Stream { device: Some(device), stop, render: Some(render), asio };
        stream
            .device
            .as_ref()
            .map_or(Ok(()), DeviceStream::play)
            .map_err(|e| format!("could not start audio output {}: {e}", self.device_name))?;
        Ok(stream)
    }
}

/// A ring for blocks of `block` frames: room for the target and a device buffer more.
fn ring(block: usize) -> (rtrb::Producer<f32>, Ring) {
    rtrb::RingBuffer::<f32>::new((ring_target(block, MAX_DEVICE_FRAMES) + block) * 2)
}

/// The render thread: runs the engine a block at a time whenever the ring holds less than its
/// target, until told to stop; then gives the renderer back. Real-time: at the highest priority
/// the system allows, and never allocating after its scratch block.
fn render_ahead(
    mut renderer: Renderer,
    mut ring: rtrb::Producer<f32>,
    frames: usize,
    stop: &AtomicBool,
    progress: &Progress,
) -> Renderer {
    // `LOOPWERK_NO_REALTIME=1` leaves the thread at its normal priority, to compare.
    if !std::env::var_os("LOOPWERK_NO_REALTIME").is_some_and(|v| v == "1") {
        let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max);
    }
    let mut block = vec![0.0f32; frames * 2];
    let capacity = ring.buffer().capacity();
    while !stop.load(Ordering::Acquire) {
        let queued = (capacity - ring.slots()) / 2;
        let target = ring_target(frames, progress.callback_frames());
        if queued >= target || ring.slots() < block.len() {
            std::thread::park_timeout(RENDER_WAIT);
            continue;
        }
        renderer.process(&mut block);
        if let Ok(chunk) = ring.write_chunk_uninit(block.len()) {
            chunk.fill_from_iter(block.iter().copied());
        }
    }
    renderer
}

/// A stream playing a project with its transport stopped, and what it needs to be heard.
pub struct Stopped {
    pub controller: Controller,
    pub output: Output,
    pub progress: Arc<Progress>,
    pub stream: Stream,
}

/// Opens the default output and plays `project` on it with the transport stopped.
pub fn start_stopped(project: &Project, audio: &AudioPool) -> Result<Stopped, String> {
    let output = Output::open()?;
    let mut renderer = Renderer::stopped(project, audio, output.sample_rate).map_err(|e| e.to_string())?;
    let mut controller = renderer.controller();
    let progress = Arc::new(Progress::default());
    let stream = output.start(renderer, &mut controller, &progress)?;
    Ok(Stopped { controller, output, progress, stream })
}

/// Opens the device for `choice` and its stream: as chosen, then shared instead of exclusive,
/// then the default output, saying why in the notices. The stream does not play yet.
fn open_fitting(choice: &OutputChoice) -> Result<(Output, DeviceStream, rtrb::Producer<f32>, Arc<Progress>), String> {
    let mut tries = vec![choice.clone()];
    if choice.exclusive {
        tries.push(OutputChoice { exclusive: false, ..choice.clone() });
    }
    if *choice != OutputChoice::default() {
        tries.push(OutputChoice::default());
    }
    let mut notices = Vec::new();
    let mut last = String::from("no audio output");
    for (i, attempt) in tries.iter().enumerate() {
        let output = match Output::choose(attempt) {
            Ok(output) => output,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let progress = Arc::new(Progress::default());
        let (producer, consumer) = ring(output.block);
        match output.open_device(consumer, &progress) {
            Ok(device) => {
                let mut output = output;
                notices.append(&mut output.notices);
                output.notices = notices;
                return Ok((output, device, producer, progress));
            }
            Err(e) => {
                let next = match tries.get(i + 1) {
                    Some(next) if !next.exclusive && attempt.exclusive => "it plays shared",
                    Some(_) => "the default output plays",
                    None => "",
                };
                if !next.is_empty() {
                    notices.push(format!("{} could not be opened ({e}); {next}", output.device_name));
                }
                last = e;
            }
        }
    }
    Err(last)
}

/// What keeps a player's output open: closing it ends the stream on the owner's thread, which
/// hands back the renderer's plug-ins.
struct Owner {
    close: mpsc::Sender<()>,
    back: mpsc::Receiver<(u32, Option<Plugins>)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Owner {
    /// Closes the stream and returns its sample rate and plug-ins, when they come back in
    /// time.
    fn close(mut self) -> Option<(u32, Plugins)> {
        let _closed = self.close.send(());
        let back = self.back.recv_timeout(CLOSE_TIMEOUT).ok();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        back.and_then(|(rate, plugins)| plugins.map(|p| (rate, p)))
    }
}

/// What a playing output runs with, for `audio.info`.
struct Live {
    progress: Arc<Progress>,
    host: String,
    device: String,
    sample_rate: u32,
    block: usize,
    exclusive: bool,
}

impl LiveOutput for Live {
    fn info(&self) -> OutputInfo {
        OutputInfo {
            host: self.host.clone(),
            device: self.device.clone(),
            sample_rate: self.sample_rate,
            buffer_frames: u32::try_from(self.progress.callback_frames()).unwrap_or(u32::MAX),
            engine_block: u32::try_from(self.block).unwrap_or(u32::MAX),
            exclusive: self.exclusive,
            latency_ms: self.progress.latency().as_secs_f64() * 1000.0,
            underruns: self.progress.underruns(),
            failed: self.progress.failed(),
        }
    }
}

/// Plays `project` on the default output with its transport stopped, from a thread that owns
/// the audio stream (a stream cannot move between threads on every platform). Returns a player
/// for the session and a description of the device; dropping the player closes the stream.
pub fn open_player(project: &Project, audio: &AudioPool) -> Result<(Player, String), String> {
    let opened = open_chosen(project, audio, &OutputChoice::default(), None)?;
    let description = opened.player.output_info().map(|i| i.describe()).unwrap_or_default();
    Ok((opened.player, description))
}

/// Plays `project` with its transport stopped on the output `choice` names (see
/// [`AudioSystem::open`]), from a thread that owns the stream. `previous` is closed first; its
/// plug-ins carry over when the sample rate stays the same.
pub fn open_chosen(
    project: &Project,
    audio: &AudioPool,
    choice: &OutputChoice,
    previous: Option<Player>,
) -> Result<Opened, String> {
    let carried = previous.and_then(|player| {
        let (controller, keep) = player.into_parts();
        let owner = keep.and_then(|k| k.downcast::<Owner>().ok());
        let back = owner.and_then(|o| o.close());
        drop(controller);
        back
    });
    let (project, audio, choice) = (project.clone(), audio.clone(), choice.clone());
    let (ready, opened) = mpsc::channel();
    let (close, closed) = mpsc::channel::<()>();
    let (back_tx, back) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("loopwerk-audio".into())
        .spawn(move || {
            let started = (|| {
                let (output, device, producer, progress) = open_fitting(&choice)?;
                let (plugins, kept) = match carried {
                    Some((rate, plugins)) if rate == output.sample_rate => (plugins, true),
                    _ => (Plugins::new(), false),
                };
                let mut renderer = Renderer::stopped_with_plugins(&project, &audio, output.sample_rate, plugins)
                    .map_err(|e| e.to_string())?;
                let mut controller = renderer.controller();
                controller
                    .run_stretch_worker(&mut renderer)
                    .map_err(|e| format!("could not start the stretch worker: {e}"))?;
                let stream = output.play(device, renderer, producer, &progress)?;
                Ok::<_, String>((output, controller, progress, stream, kept))
            })();
            match started {
                Ok((output, controller, progress, stream, kept)) => {
                    let live = Live {
                        progress,
                        host: output.host.clone(),
                        device: output.device_name.clone(),
                        sample_rate: output.sample_rate,
                        block: output.block,
                        exclusive: output.exclusive,
                    };
                    let notice = (!output.notices.is_empty()).then(|| output.notices.join("; "));
                    if ready.send(Ok((controller, live, notice, kept))).is_ok() {
                        // Returns once the player, which holds `close`, closes or is dropped.
                        let _closed = closed.recv();
                    }
                    let plugins = stream.close().map(Renderer::into_plugins);
                    let _unheard = back_tx.send((output.sample_rate, plugins));
                }
                Err(e) => {
                    let _unheard = ready.send(Err(e));
                }
            }
        })
        .map_err(|e| format!("could not start the audio thread: {e}"))?;
    let (controller, live, notice, plugins_kept) =
        opened.recv().map_err(|_| "the audio thread stopped".to_string())??;
    let owner = Owner { close, back, thread: Some(thread) };
    let player = Player::with_output(controller, Some(Box::new(owner)), Arc::new(live));
    Ok(Opened { player, notice, plugins_kept })
}

/// The system's audio devices for the session ([`AudioSystem`]): the hosts cpal offers here
/// and opening a project on the device chosen.
#[derive(Default)]
pub struct DeviceOutput {
    /// The last listing, for the ASIO host while its driver is in use.
    listed: std::sync::Mutex<Vec<HostInfo>>,
}

impl DeviceOutput {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioSystem for DeviceOutput {
    fn hosts(&self) -> Vec<HostInfo> {
        let mut hosts = devices::list_hosts();
        let Ok(mut listed) = self.listed.lock() else { return hosts };
        if ASIO_OPEN.load(Ordering::Acquire)
            && let Some(before) = listed.iter().find(|h| h.name == "ASIO")
            && let Some(now) = hosts.iter_mut().find(|h| h.name == "ASIO")
            && now.devices.len() < before.devices.len()
        {
            *now = before.clone();
        }
        listed.clone_from(&hosts);
        hosts
    }

    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String> {
        open_chosen(project, audio, choice, previous)
    }
}

/// The device's stream: its callback takes what the render thread put in the ring, and
/// silence (counted as an underrun) for what is not there yet, and measures the latency.
fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut ring: Ring,
    progress: &Arc<Progress>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    let mut scratch = vec![0.0f32; MAX_DEVICE_FRAMES * 2];
    let on_data = Arc::clone(progress);
    let on_error = Arc::clone(progress);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let timestamp = info.timestamp();
                let device_delay = timestamp.playback.duration_since(timestamp.callback);
                let queued = ring.slots() / 2;
                on_data.set_callback_frames(data.len() / channels.max(1));
                on_data.set_latency(measure_latency(queued, device_delay, sample_rate));
                for block in data.chunks_mut(MAX_DEVICE_FRAMES * channels) {
                    let frames = block.len() / channels;
                    let Some(stereo) = scratch.get_mut(..frames * 2) else { return };
                    read_ring(&mut ring, stereo, &on_data);
                    write_frames(block, channels, stereo);
                    on_data.add_frames(frames);
                }
            },
            move |_err| on_error.fail(),
            None,
        )
        .map_err(|e| format!("could not open audio output {device}: {e}"))
}

/// Writes interleaved stereo into a device buffer with any channel count: mono gets the average,
/// channels beyond the first two get silence.
fn write_frames<T: SizedSample + FromSample<f32>>(out: &mut [T], channels: usize, stereo: &[f32]) {
    for (frame, [left, right]) in out.chunks_mut(channels).zip(stereo.as_chunks::<2>().0) {
        let mut samples = frame.iter_mut();
        if channels == 1 {
            if let Some(mono) = samples.next() {
                *mono = T::from_sample((left + right) * 0.5);
            }
            continue;
        }
        if let Some(l) = samples.next() {
            *l = T::from_sample(*left);
        }
        if let Some(r) = samples.next() {
            *r = T::from_sample(*right);
        }
        for rest in samples {
            *rest = T::from_sample(0.0f32);
        }
    }
}

#[cfg(test)]
mod tests;
