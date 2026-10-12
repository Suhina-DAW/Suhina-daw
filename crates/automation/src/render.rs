//! Offline rendering to WAV files, shared by the CLI and the MCP server.

use std::path::Path;

use loopwerk_engine::model::{Arrangement, ArrangementLane, Project};
use loopwerk_engine::{AudioPool, Renderer, render_offline_with_audio};
use loopwerk_session::{CommandKind, Player, Session, commands};
use serde_json::{Value, json};

/// Longest launch-script render, in seconds.
pub const MAX_SCRIPT_SECONDS: f64 = 600.0;
/// Most steps in one launch script.
pub const MAX_SCRIPT_STEPS: usize = 1024;

/// What a render produced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSummary {
    pub seconds: f64,
    pub sample_rate: u32,
    /// Largest absolute sample value; above 1.0 means the file clips.
    pub peak: f32,
}

/// Renders the project's arrangement, with the audio files in `audio`, to a 32-bit float stereo
/// WAV file at `path`.
pub fn render_wav(
    project: &Project,
    audio: &AudioPool,
    path: &Path,
    sample_rate: u32,
) -> Result<RenderSummary, String> {
    let samples = render_offline_with_audio(project, audio, sample_rate, 512).map_err(|e| e.to_string())?;
    let spec =
        hound::WavSpec { channels: 2, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("{}: {e}", path.display()))?;
    for sample in &samples {
        writer.write_sample(*sample).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    writer.finalize().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(RenderSummary {
        seconds: samples.len() as f64 / 2.0 / f64::from(sample_rate),
        sample_rate,
        peak: samples.iter().fold(0.0f32, |m, s| m.max(s.abs())),
    })
}

/// One step of a launch script: the command `id` with `params`, run when the render reaches
/// `at` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchStep {
    pub at: f64,
    pub id: String,
    pub params: Value,
}

/// Renders `seconds` of the project to a WAV file at `path`, starting with the transport
/// stopped and running each step's command when the render reaches its time, as if it were
/// typed into a live session at that moment. Only transport and read commands may be used.
/// Returns the summary and each step's result.
pub fn render_launch_script(
    project: &Project,
    audio: &AudioPool,
    path: &Path,
    sample_rate: u32,
    steps: &[LaunchStep],
    seconds: f64,
) -> Result<(RenderSummary, Vec<Value>), String> {
    if !(seconds.is_finite() && seconds > 0.0 && seconds <= MAX_SCRIPT_SECONDS) {
        return Err(format!("seconds must be more than 0 and at most {MAX_SCRIPT_SECONDS}, got {seconds}"));
    }
    if steps.len() > MAX_SCRIPT_STEPS {
        return Err(format!("a launch script may have at most {MAX_SCRIPT_STEPS} steps, got {}", steps.len()));
    }
    for step in steps {
        let kind = commands().iter().find(|c| c.id == step.id).map(|c| c.kind);
        match kind {
            Some(CommandKind::Transport | CommandKind::Read) => {}
            Some(_) => return Err(format!("{}: a launch script may only run transport and read commands", step.id)),
            None => return Err(format!("unknown command {}; command_list shows them", step.id)),
        }
        if !(step.at.is_finite() && step.at >= 0.0) {
            return Err(format!("{}: at must be a time in seconds, at least 0", step.id));
        }
    }
    let mut steps: Vec<&LaunchStep> = steps.iter().collect();
    steps.sort_by(|a, b| a.at.total_cmp(&b.at));

    let mut renderer = Renderer::stopped(project, audio, sample_rate).map_err(|e| e.to_string())?;
    let mut session = Session::new(project.clone());
    session.attach_player(Player::new(renderer.controller(), None));

    let spec =
        hound::WavSpec { channels: 2, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("{}: {e}", path.display()))?;
    let frame_of = |at: f64| (at * f64::from(sample_rate)).round() as u64;
    let total = frame_of(seconds);
    let mut block = vec![0.0f32; 2 * 512];
    let mut done = 0u64;
    let mut peak = 0.0f32;
    let mut results = Vec::new();
    let mut next = steps.into_iter().peekable();
    while done < total {
        while let Some(step) = next.next_if(|s| frame_of(s.at) <= done) {
            results.push(match session.execute(&step.id, &step.params) {
                Ok(result) => json!({ "at": step.at, "id": step.id, "ok": true, "result": result }),
                Err(e) => json!({ "at": step.at, "id": step.id, "ok": false, "error": e.0 }),
            });
        }
        let until = next.peek().map_or(total, |s| frame_of(s.at)).clamp(done + 1, total).min(done + 512);
        let frames = usize::try_from(until - done).unwrap_or(512);
        let Some(out) = block.get_mut(..2 * frames) else { break };
        renderer.process(out);
        for sample in out.iter() {
            peak = peak.max(sample.abs());
            writer.write_sample(*sample).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        done = until;
    }
    for step in next {
        results.push(json!({ "at": step.at, "id": step.id, "ok": false, "error": "after the end of the render" }));
    }
    writer.finalize().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((RenderSummary { seconds: done as f64 / f64::from(sample_rate), sample_rate, peak }, results))
}

/// Replaces the arrangement with the clips of one launcher scene (0-based), each played once
/// from the start, as if the scene were launched at time zero.
pub fn scene_as_arrangement(project: &Project, scene: usize) -> Result<Project, String> {
    let count = project.launcher.scenes.len();
    if scene >= count {
        return Err(format!("scene {} does not exist; the project has {count} scenes", scene.saturating_add(1)));
    }
    let lanes = project
        .launcher
        .columns
        .iter()
        .map(|column| ArrangementLane {
            name: String::new(),
            track: Some(column.track),
            clips: column.slots.get(scene).and_then(|s| s.clip.clone()).into_iter().collect(),
        })
        .collect();
    let mut project = project.clone();
    project.arrangement = Arrangement { lanes };
    Ok(project)
}

#[cfg(test)]
mod tests {
    use loopwerk_engine::model::*;

    use super::*;

    const SR: u32 = 48_000;

    /// One track whose only slot loops a short note on every beat, at 120 bpm.
    fn project() -> Project {
        Project {
            tracks: vec![Track {
                id: TrackId(1),
                name: "Drums".into(),
                color: None,
                content: ContentKinds { notes: true, audio: false },
                channel: Some(ChannelId(1)),
                parent: None,
                extensions: Extensions::default(),
            }],
            channels: vec![Channel {
                id: ChannelId(1),
                name: "Drums".into(),
                role: ChannelRole::Regular,
                volume: 1.0,
                pan: 0.0,
                mute: false,
                solo: false,
                output: None,
                sends: Vec::new(),
                devices: Vec::new(),
                extensions: Extensions::default(),
            }],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "Beat".into(),
                length: 1.0,
                lanes: vec![ContentLane {
                    target: None,
                    data: LaneData::Notes(vec![Note {
                        time: 0.0,
                        duration: 0.25,
                        key: 36,
                        velocity: 1.0,
                        release_velocity: 0.5,
                        channel: 0,
                    }]),
                }],
                extensions: Extensions::default(),
            }],
            launcher: Launcher {
                scenes: vec![Scene::default()],
                columns: vec![LauncherColumn {
                    track: TrackId(1),
                    slots: vec![Slot {
                        clip: Some(Clip {
                            content: ContentId(1),
                            start: 0.0,
                            length: 1.0,
                            offset: 0.0,
                            loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
                            enabled: true,
                            fade_in: 0.0,
                            fade_out: 0.0,
                            extensions: Extensions::default(),
                        }),
                        has_stop: true,
                    }],
                }],
            },
            ..Project::default()
        }
    }

    fn step(at: f64, id: &str, params: Value) -> LaunchStep {
        LaunchStep { at, id: id.into(), params }
    }

    fn temp_wav(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("loopwerk-render-{name}-{}.wav", std::process::id()))
    }

    /// Frames where sound starts after more than 100 silent frames, in the left channel.
    fn onsets(path: &Path) -> Vec<usize> {
        let mut reader = hound::WavReader::open(path).unwrap();
        let left: Vec<f32> = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
        let mut result = Vec::new();
        let mut silent_run = usize::MAX;
        for (i, s) in left.iter().enumerate() {
            let loud = s.abs() > 1e-6;
            if loud && silent_run > 100 {
                result.push(i);
            }
            silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
        }
        result
    }

    #[test]
    fn a_launch_script_renders_launches_at_their_times() {
        let path = temp_wav("script");
        let steps = [
            step(0.1, "clip.launch", json!({ "track": "Drums", "scene": 1 })),
            step(0.3, "track.stop", json!({ "track": "Drums" })),
            step(0.31, "launcher.state", json!({})),
            step(9.0, "transport.stop", json!({})),
        ];
        let (summary, results) = render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 3.0).unwrap();
        assert!((summary.seconds - 3.0).abs() < 1e-9);
        assert!(summary.peak > 0.1);
        // Launched at once (the transport was stopped), so the bar starts at 0.1 s; the stop waits
        // for the next bar, 2.1 s.
        let start = SR as usize / 10;
        let beat = SR as usize / 2;
        let expected = [start, start + beat, start + 2 * beat, start + 3 * beat];
        let actual = onsets(&path);
        assert_eq!(actual.len(), 4, "{actual:?}");
        assert!(actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2), "{actual:?}");
        assert_eq!(results.len(), 4);
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[2]["result"]["tracks"][0]["queued"], "stop");
        assert_eq!(results[3]["ok"], false);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn launch_scripts_take_only_transport_and_read_commands() {
        let path = temp_wav("bad");
        let run = |steps: &[LaunchStep], seconds: f64| {
            render_launch_script(&project(), &AudioPool::new(), &path, SR, steps, seconds).map(|_| ())
        };
        assert!(run(&[step(0.0, "transport.setTempo", json!({ "bpm": 90 }))], 1.0).is_err());
        assert!(run(&[step(0.0, "edit.undo", json!({}))], 1.0).is_err());
        assert!(run(&[step(0.0, "nope", json!({}))], 1.0).is_err());
        assert!(run(&[step(f64::NAN, "transport.play", json!({}))], 1.0).is_err());
        assert!(run(&[step(-1.0, "transport.play", json!({}))], 1.0).is_err());
        for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_SCRIPT_SECONDS + 1.0] {
            assert!(run(&[], seconds).is_err(), "{seconds}");
        }
        // A failing command is reported in its step, not as a failed render.
        let steps = [step(0.0, "clip.launch", json!({ "track": "Drums", "scene": 5 }))];
        let (_, results) = render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 0.1).unwrap();
        assert_eq!(results[0]["ok"], false);
        let _ = std::fs::remove_file(&path);
    }
}
