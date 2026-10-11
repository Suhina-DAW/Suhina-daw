//! Real-time playback on the default audio output.
//!
//! The engine's [`Renderer`] is built here, on the main thread, and then moved into the audio
//! callback. The callback only calls `Renderer::process`, converts samples and sets atomics: no
//! allocation, locks or I/O on the audio thread (AGENTS.md rule 3). Launcher commands reach it
//! through the renderer's lock-free command queue.

use std::io::BufRead;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use loopwerk_engine::model::{LaneData, Project};
use loopwerk_engine::{AudioPool, Renderer};
use loopwerk_session::{Player, Session};
use serde_json::{Value, json};

/// Frames rendered per engine call; output buffers larger than this are filled in several calls.
const SCRATCH_FRAMES: usize = 4096;
/// Extra time to wait after the last frame, so the device can drain its buffer.
const TAIL: Duration = Duration::from_millis(300);

/// State the audio callback shares with the main thread.
#[derive(Default)]
struct Progress {
    frames: AtomicU64,
    stream_error: AtomicBool,
}

/// The default output device and the format it asks for.
struct Output {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
}

impl Output {
    fn open() -> Result<Self, String> {
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

    /// Starts playing `renderer` on the device.
    fn start(&self, renderer: Renderer, progress: &Arc<Progress>) -> Result<cpal::Stream, String> {
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

    fn describe(&self) -> String {
        format!("{} ({} Hz, {} channels, {})", self.device, self.config.sample_rate, self.config.channels, self.format)
    }
}

/// Plays the project's arrangement once on the default output device and returns when it ends.
pub fn play(project: &Project, audio: &AudioPool) -> Result<(), String> {
    let output = Output::open()?;
    let renderer = Renderer::with_audio(project, audio, output.config.sample_rate).map_err(|e| e.to_string())?;
    let length = renderer.length();
    if length == 0 {
        println!("{}", nothing_to_play(project));
        return Ok(());
    }
    let progress = Arc::new(Progress::default());
    let stream = output.start(renderer, &progress)?;

    let seconds = length as f64 / f64::from(output.config.sample_rate);
    println!("playing {seconds:.2} s on {}; Ctrl+C stops", output.describe());

    // Wait for the callback to reach the end, with a wall-clock limit in case the device stalls.
    let deadline = Instant::now() + Duration::from_secs_f64(seconds) + Duration::from_secs(5);
    while progress.frames.load(Ordering::Relaxed) < length {
        if progress.stream_error.load(Ordering::Relaxed) {
            return Err(format!("audio output {} reported an error during playback", output.device));
        }
        if Instant::now() > deadline {
            return Err(format!("audio output {} stopped advancing", output.device));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(TAIL);
    drop(stream);
    println!("done");
    Ok(())
}

const LAUNCHER_HELP: &str = "\
commands (scenes count from 1; a track is its name or id):
  scene <n>             launch scene n
  clip <track> <n>      launch the clip in scene n of a track
  stop <track>          stop a track's clip
  stopall               stop every clip
  play / stop           start the transport from the beginning / stop everything now
  quant <value>         launch quantisation: none, 8 bars ... 1 bar, 1/2 ... 1/32, triplets as 1/8T
  back                  tracks that launched clips play their arrangement again
  state                 what is playing and queued
  grid                  the scenes and each track's clips
  <command.id> [json]   any command, e.g. track.setMute {\"track\": \"Bass\", \"mute\": true}
  help, quit";

/// Plays the project on the default output with the transport stopped and reads launcher
/// commands from standard input until `quit` or end of input.
pub fn launcher(project: Project, audio: &AudioPool) -> Result<(), String> {
    if project.launcher.columns.iter().all(|c| c.slots.iter().all(|s| s.clip.is_none())) {
        println!("note: the project has no clips in its clip launcher; only `play` (the arrangement) will sound");
    }
    let output = Output::open()?;
    let mut renderer = Renderer::stopped(&project, audio, output.config.sample_rate).map_err(|e| e.to_string())?;
    let controller = renderer.controller();
    let progress = Arc::new(Progress::default());
    let stream = output.start(renderer, &progress)?;

    let mut session = Session::new(project);
    session.attach_player(Player::new(controller, None));
    println!("clip launcher on {}\n{LAUNCHER_HELP}\n", output.describe());
    print_grid(&mut session);

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("reading commands: {e}"))?;
        if progress.stream_error.load(Ordering::Relaxed) {
            return Err(format!("audio output {} reported an error during playback", output.device));
        }
        match parse_line(&line) {
            Ok(Line::Empty) => {}
            Ok(Line::Quit) => break,
            Ok(Line::Help) => println!("{LAUNCHER_HELP}"),
            Ok(Line::Command(id, params)) => match session.execute(&id, &params) {
                Ok(result) if id == "launcher.state" => print_state(&result),
                Ok(result) if id == "project.launcher" => print_grid_json(&result),
                Ok(result) => println!("{id}: {result}"),
                Err(e) => println!("error: {e}"),
            },
            Err(e) => println!("error: {e} (help lists the commands)"),
        }
    }
    // Stop, then let the release tails and fades finish before closing the device.
    if let Err(e) = session.execute("transport.stop", &json!({})) {
        eprintln!("warning: {e}");
    }
    std::thread::sleep(TAIL);
    drop(stream);
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Line {
    Empty,
    Quit,
    Help,
    Command(String, Value),
}

/// A track argument: a number is a track id, anything else a name.
fn track_value(words: &[&str]) -> Value {
    let text = words.join(" ");
    text.parse::<u64>().map_or_else(|_| json!(text), |id| json!(id))
}

fn scene_value(word: &str) -> Result<Value, String> {
    word.parse::<u64>().map(|n| json!(n)).map_err(|_| format!("{word:?} is not a scene number"))
}

/// Turns one line of input into a command id and parameters.
fn parse_line(line: &str) -> Result<Line, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let command = |id: &str, params: Value| Ok(Line::Command(id.to_string(), params));
    match words.as_slice() {
        [] => Ok(Line::Empty),
        ["quit" | "exit"] => Ok(Line::Quit),
        ["help" | "?"] => Ok(Line::Help),
        ["play"] => command("transport.play", json!({})),
        ["stop"] => command("transport.stop", json!({})),
        ["stopall"] => command("launcher.stopAll", json!({})),
        ["back"] => command("launcher.backToArrangement", json!({})),
        ["state" | "s"] => command("launcher.state", json!({})),
        ["grid"] => command("project.launcher", json!({})),
        ["scene", n] => command("scene.launch", json!({ "scene": scene_value(n)? })),
        ["clip", track @ .., n] if !track.is_empty() => {
            command("clip.launch", json!({ "track": track_value(track), "scene": scene_value(n)? }))
        }
        ["stop", track @ ..] => command("track.stop", json!({ "track": track_value(track) })),
        ["quant", value @ ..] if !value.is_empty() => {
            command("launcher.setQuantization", json!({ "value": value.join(" ") }))
        }
        [id, ..] if id.contains('.') => {
            let rest = line.trim_start().get(id.len()..).unwrap_or_default().trim();
            let params = if rest.is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| format!("parameters are not JSON: {e}"))?
            };
            command(id, params)
        }
        _ => Err(format!("unknown command {line:?}")),
    }
}

fn print_grid(session: &mut Session) {
    if let Ok(grid) = session.execute("project.launcher", &json!({})) {
        print_grid_json(&grid);
    }
}

fn print_grid_json(grid: &Value) {
    let empty = Vec::new();
    let scenes = grid["scenes"].as_array().unwrap_or(&empty);
    println!("{} scenes", scenes.len());
    for scene in scenes {
        let name = scene["name"].as_str().unwrap_or_default();
        let tempo = scene["tempo_bpm"].as_f64().map(|t| format!(" ({t} bpm)")).unwrap_or_default();
        if !name.is_empty() || !tempo.is_empty() {
            println!("  scene {}: {name}{tempo}", scene["scene"]);
        }
    }
    println!("clips by track (scene=clip):");
    for track in grid["tracks"].as_array().unwrap_or(&empty) {
        let clips: Vec<String> = track["slots"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|s| {
                let name = s["clip"].as_str()?;
                let name = if name.is_empty() { "unnamed" } else { name };
                let off = if s["enabled"] == false { " (deactivated)" } else { "" };
                Some(format!("{}={name}{off}", s["scene"]))
            })
            .collect();
        let clips = if clips.is_empty() { "-".to_string() } else { clips.join(", ") };
        println!("  {}: {clips}", track["track_name"].as_str().unwrap_or_default());
    }
}

fn print_state(state: &Value) {
    let transport = if state["running"] == true { "playing" } else { "stopped" };
    println!("{transport} at {} ({} bpm)", state["position"].as_str().unwrap_or("?"), state["tempo_bpm"]);
    let empty = Vec::new();
    for track in state["tracks"].as_array().unwrap_or(&empty) {
        let describe = |v: &Value| match v {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            v => Some(format!("{} (scene {})", v["clip"].as_str().unwrap_or("?"), v["scene"])),
        };
        let playing = describe(&track["playing"]).unwrap_or_else(|| "-".into());
        let queued = describe(&track["queued"]).map(|q| format!(", next: {q}")).unwrap_or_default();
        println!("  {}: {playing}{queued}", track["track_name"].as_str().unwrap_or_default());
    }
}

/// Explains why a project produced no sound: no clips at all, clips without notes, or audio
/// clips whose files are missing.
fn nothing_to_play(project: &Project) -> String {
    let clips: Vec<_> = project.arrangement.lanes.iter().flat_map(|l| &l.clips).collect();
    let audio = clips
        .iter()
        .filter(|c| {
            project
                .content(c.content)
                .is_some_and(|content| content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))))
        })
        .count();
    match (clips.len(), audio) {
        (0, _) => {
            "nothing to play: the arrangement has no clips (try --scene <n>, or --launcher to launch clips)".to_string()
        }
        (_, 0) => "nothing to play: the arrangement's clips have no notes".to_string(),
        (total, audio) => format!(
            "nothing to play: {audio} of {total} arrangement clips are audio clips whose files could not be \
             loaded (see the warnings above)"
        ),
    }
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

    #[test]
    fn launcher_lines_become_commands() {
        let cmd = |id: &str, params: Value| Ok(Line::Command(id.into(), params));
        assert_eq!(parse_line("  "), Ok(Line::Empty));
        assert_eq!(parse_line("quit"), Ok(Line::Quit));
        assert_eq!(parse_line("scene 2"), cmd("scene.launch", json!({ "scene": 2 })));
        assert_eq!(parse_line("clip Lead Vox 3"), cmd("clip.launch", json!({ "track": "Lead Vox", "scene": 3 })));
        assert_eq!(parse_line("clip 4 1"), cmd("clip.launch", json!({ "track": 4, "scene": 1 })));
        assert_eq!(parse_line("stop"), cmd("transport.stop", json!({})));
        assert_eq!(parse_line("stop Drums"), cmd("track.stop", json!({ "track": "Drums" })));
        assert_eq!(parse_line("quant 2 bars"), cmd("launcher.setQuantization", json!({ "value": "2 bars" })));
        assert_eq!(
            parse_line("track.setMute {\"track\": 1, \"mute\": true}"),
            cmd("track.setMute", json!({ "track": 1, "mute": true }))
        );
        assert_eq!(parse_line("project.info"), cmd("project.info", json!({})));
        assert!(parse_line("scene two").is_err());
        assert!(parse_line("clip 3").is_err());
        assert!(parse_line("track.setMute {nope").is_err());
        assert!(parse_line("dance").is_err());
    }
}
