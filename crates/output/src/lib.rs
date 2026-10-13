//! The audio device: plays an engine [`Renderer`] on the default output (cpal).
//!
//! The renderer is built on the main thread and then moved into the audio callback. The callback
//! only calls `Renderer::process`, converts samples and sets atomics: no allocation, locks or I/O
//! on the audio thread (AGENTS.md rule 3). Launcher commands reach it through the renderer's
//! lock-free command queue, and stretched clips are rendered ahead on a worker thread of their
//! own (ADR 0003).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use suhina_engine::model::Project;
use suhina_engine::{AudioPool, Controller, Renderer};
use suhina_session::Player;

/// Frames rendered per engine call; output buffers larger than this are filled in several calls.
const SCRATCH_FRAMES: usize = 4096;

/// State the audio callback shares with the main thread.
#[derive(Default)]
pub struct Progress {
    frames: AtomicU64,
    stream_error: AtomicBool,
}

impl Progress {
    /// Frames the device has been given so far.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Whether the device reported an error.
    pub fn failed(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
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

    /// Starts playing `renderer` on the device, with its stretched clips rendered ahead on a
    /// worker thread that `controller` keeps (and stops when it is dropped). The stream plays
    /// until it is dropped.
    pub fn start(
        &self,
        mut renderer: Renderer,
        controller: &mut Controller,
        progress: &Arc<Progress>,
    ) -> Result<cpal::Stream, String> {
        controller.run_stretch_worker(&mut renderer).map_err(|e| format!("could not start the stretch worker: {e}"))?;
        let (device, config) = (&self.device, self.config);
        let stream = match self.format {
            cpal::SampleFormat::F32 => build::<f32>(device, config, renderer, progress),
            cpal::SampleFormat::I16 => build::<i16>(device, config, renderer, progress),
            cpal::SampleFormat::I32 => build::<i32>(device, config, renderer, progress),
            cpal::SampleFormat::U16 => build::<u16>(device, config, renderer, progress),
            other => {
                return Err(format!("audio output {device} uses sample format {other}, which is not supported yet"));
            }
        }?;
        stream.play().map_err(|e| format!("could not start audio output {device}: {e}"))?;
        Ok(stream)
    }
}

/// A stream playing a project with its transport stopped, and what it needs to be heard.
pub struct Stopped {
    pub controller: Controller,
    pub output: Output,
    pub progress: Arc<Progress>,
    pub stream: cpal::Stream,
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

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut renderer: Renderer,
    progress: &Arc<Progress>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * 2];
    let on_data = Arc::clone(progress);
    let on_error = Arc::clone(progress);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for block in data.chunks_mut(SCRATCH_FRAMES * channels) {
                    let frames = block.len() / channels;
                    let Some(stereo) = scratch.get_mut(..frames * 2) else { return };
                    renderer.process(stereo);
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
