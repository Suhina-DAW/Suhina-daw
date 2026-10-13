//! The audio device: plays an engine [`Renderer`] on the default output (cpal).
//!
//! The renderer is built on the main thread and then moved to a render thread of its own,
//! which runs it ahead of the device in blocks of [`ENGINE_BLOCK`] frames into a lock-free
//! ring ([`rtrb`]); the device's callback only copies from the ring, converts samples and sets
//! atomics: no allocation, locks or I/O on either thread (AGENTS.md rule 3). Rendering in
//! blocks of its own, rather than in the device's buffers, gives the plug-ins of a block the
//! same time whatever the device's buffer size (some drivers give a few milliseconds), at the
//! cost of the ring's latency. Launcher commands reach the renderer through its lock-free
//! command queue, and stretched clips are rendered ahead on a worker thread of their own (ADR
//! 0003).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use suhina_engine::model::Project;
use suhina_engine::{AudioPool, Controller, Renderer};
use suhina_session::Player;

/// Frames the engine renders per block, ahead of the device. The plug-ins of a block get a
/// share of this block's playing time (ADR 0006), whatever the device's buffer size.
pub const ENGINE_BLOCK: usize = 1024;
/// Blocks the ring holds: the render thread keeps this much ahead of the device, so the
/// latency from the engine to the device is up to this many blocks.
const RING_BLOCKS: usize = 2;
/// How long the render thread waits when the ring is full before looking again.
const RENDER_WAIT: Duration = Duration::from_millis(1);

/// State the audio callback shares with the main thread.
#[derive(Default)]
pub struct Progress {
    frames: AtomicU64,
    underruns: AtomicU64,
    stream_error: AtomicBool,
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
}

/// A playing stream: the device's, and the render thread that feeds it. Dropping it stops
/// both.
pub struct Stream {
    _device: cpal::Stream,
    stop: Arc<AtomicBool>,
    render: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(render) = self.render.take() {
            let _ = render.join();
        }
    }
}

/// The default output device and the format it asks for.
pub struct Output {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
}

impl Output {
    pub fn open() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device found")?;
        let supported = device.default_output_config().map_err(|e| format!("audio output {device}: {e}"))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        if config.channels == 0 {
            return Err(format!("audio output {device} reports no channels"));
        }
        Ok(Self { device, config, format })
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// The device's name, sample rate, channels and sample format, for messages.
    pub fn describe(&self) -> String {
        format!("{} ({} Hz, {} channels, {})", self.device, self.config.sample_rate, self.config.channels, self.format)
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
        controller.run_stretch_worker(&mut renderer).map_err(|e| format!("could not start the stretch worker: {e}"))?;
        let (device, config) = (&self.device, self.config);
        let block = engine_block();
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(block * 2 * RING_BLOCKS);
        let stream = match self.format {
            cpal::SampleFormat::F32 => build::<f32>(device, config, consumer, progress),
            cpal::SampleFormat::I16 => build::<i16>(device, config, consumer, progress),
            cpal::SampleFormat::I32 => build::<i32>(device, config, consumer, progress),
            cpal::SampleFormat::U16 => build::<u16>(device, config, consumer, progress),
            other => {
                return Err(format!("audio output {device} uses sample format {other}, which is not supported yet"));
            }
        }?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let render = std::thread::Builder::new()
            .name("suhina-render".into())
            .spawn(move || render_ahead(renderer, producer, block, &stopping))
            .map_err(|e| format!("could not start the render thread: {e}"))?;
        stream.play().map_err(|e| format!("could not start audio output {device}: {e}"))?;
        Ok(Stream { _device: stream, stop, render: Some(render) })
    }
}

/// The render thread: runs the engine a block at a time whenever the ring has room for one,
/// until told to stop. Real-time: at the highest priority the system allows, and never
/// allocating after its scratch block.
/// The frames the engine renders per block: [`ENGINE_BLOCK`], or `SUHINA_ENGINE_BLOCK`
/// (64 to 4096) to try another size.
fn engine_block() -> usize {
    std::env::var("SUHINA_ENGINE_BLOCK")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| (64..=4096).contains(n))
        .unwrap_or(ENGINE_BLOCK)
}

fn render_ahead(mut renderer: Renderer, mut ring: rtrb::Producer<f32>, frames: usize, stop: &AtomicBool) {
    // `SUHINA_NO_REALTIME=1` leaves the thread at its normal priority, to compare.
    if !std::env::var_os("SUHINA_NO_REALTIME").is_some_and(|v| v == "1") {
        let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max);
    }
    let mut block = vec![0.0f32; frames * 2];
    while !stop.load(Ordering::Acquire) {
        if ring.slots() < block.len() {
            std::thread::park_timeout(RENDER_WAIT);
            continue;
        }
        renderer.process(&mut block);
        if let Ok(chunk) = ring.write_chunk_uninit(block.len()) {
            chunk.fill_from_iter(block.iter().copied());
        }
    }
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
    let mut renderer = Renderer::stopped(project, audio, output.config.sample_rate).map_err(|e| e.to_string())?;
    let mut controller = renderer.controller();
    let progress = Arc::new(Progress::default());
    let stream = output.start(renderer, &mut controller, &progress)?;
    Ok(Stopped { controller, output, progress, stream })
}

/// Plays `project` on the default output with its transport stopped, from a thread that owns
/// the audio stream (a stream cannot move between threads on every platform). Returns a player
/// for the session and a description of the device; dropping the player closes the stream.
pub fn open_player(project: &Project, audio: &AudioPool) -> Result<(Player, String), String> {
    let (project, audio) = (project.clone(), audio.clone());
    let (ready, opened) = mpsc::channel();
    let (close, closed) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("suhina-audio".into())
        .spawn(move || match start_stopped(&project, &audio) {
            Ok(Stopped { controller, output, stream, .. }) => {
                if ready.send(Ok((controller, output.describe()))).is_ok() {
                    // Returns once the player, which holds `close`, is dropped.
                    let _closed = closed.recv();
                }
                drop(stream);
            }
            Err(e) => {
                let _unheard = ready.send(Err(e));
            }
        })
        .map_err(|e| format!("could not start the audio thread: {e}"))?;
    let (controller, description) = opened.recv().map_err(|_| "the audio thread stopped".to_string())??;
    Ok((Player::new(controller, Some(Box::new(close))), description))
}

/// The device's stream: its callback takes what the render thread put in the ring, and
/// silence (counted as an underrun) for what is not there yet.
fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut ring: rtrb::Consumer<f32>,
    progress: &Arc<Progress>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let mut scratch = vec![0.0f32; 4096 * 2];
    let on_data = Arc::clone(progress);
    let on_error = Arc::clone(progress);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for block in data.chunks_mut(4096 * channels) {
                    let frames = block.len() / channels;
                    let Some(stereo) = scratch.get_mut(..frames * 2) else { return };
                    let ready = ring.slots().min(stereo.len()) / 2 * 2;
                    match ring.read_chunk(ready) {
                        Ok(chunk) => {
                            for (sample, out) in chunk.into_iter().zip(stereo.iter_mut()) {
                                *out = sample;
                            }
                        }
                        Err(_) => stereo.fill(0.0),
                    }
                    if ready < stereo.len() {
                        if let Some(missing) = stereo.get_mut(ready..) {
                            missing.fill(0.0);
                        }
                        on_data.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                    write_frames(block, channels, stereo);
                    on_data.frames.fetch_add(frames as u64, Ordering::Relaxed);
                }
            },
            move |_err| on_error.stream_error.store(true, Ordering::Relaxed),
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
mod tests {
    use super::*;

    #[test]
    fn stereo_maps_onto_any_channel_count() {
        let stereo = [0.5f32, -0.5, 1.0, 0.0];

        let mut mono = [0.0f32; 2];
        write_frames(&mut mono, 1, &stereo);
        assert_eq!(mono, [0.0, 0.5]);

        let mut two = [0.0f32; 4];
        write_frames(&mut two, 2, &stereo);
        assert_eq!(two, stereo);

        let mut four = [9.0f32; 8];
        write_frames(&mut four, 4, &stereo);
        assert_eq!(four, [0.5, -0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);

        let mut ints = [0i16; 4];
        write_frames(&mut ints, 2, &stereo);
        assert_eq!(ints[2], i16::MAX);
    }
}
