//! Headless Loopwerk CLI.

mod play;

use std::path::PathBuf;
use std::process::ExitCode;

use loopwerk_automation::{LoopwerkMcp, Workspace, render_wav, scene_as_arrangement};
use loopwerk_engine::AudioPool;
use loopwerk_engine::model::*;

const USAGE: &str = "\
usage: loopwerk-cli render (<project.als> | --demo) -o <out.wav> [--scene <n>] [--sample-rate <hz>]
       loopwerk-cli play (<project.als> | --demo) [--scene <n> | --launcher]
       loopwerk-cli mcp [--read-root <dir>] [--write-root <dir>] [--audio]
       loopwerk-cli plugins (scan [--all] | list | folders | add-folder <dir> | remove-folder <dir>)
       loopwerk-cli plugin run <plug-in> <in-audio> <out.wav> [--tail <seconds>]

commands:
  render    render a project offline to a 32-bit float stereo WAV file
  play      play a project once on the default audio output, or with --launcher launch its
            clips and scenes by typing commands
  mcp       serve the Model Context Protocol on stdin/stdout for agents; files can be read only
            below --read-root and written only below --write-root (no root, no access); with
            --audio, opened projects play on the default output so agents can launch clips
  plugins   find installed plug-ins: scan looks in the standard folders and the added ones
            (only new and changed files, or every file with --all), list shows what was found,
            folders shows where scans look, add-folder and remove-folder change that
  plugin    plugin run: put an audio file through a plug-in (a name or id from plugins list)
            in its own process, offline, and write a 32-bit float stereo WAV; --tail adds
            that many seconds of silence for reverbs and delays to ring out (default 2)

options:
  <project.als>      a Live Set to import
  --demo             use the built-in demo project instead
  -o, --output       output WAV path (render only)
  --scene <n>        use scene n (1-based) of the clip launcher instead of the arrangement
  --launcher         play only: start stopped and read launcher commands (scene 2, clip Bass 1,
                     stop Bass, quit, help) from standard input
  --sample-rate      sample rate in Hz (render only; default 48000)";

#[derive(Debug)]
enum Source {
    Demo,
    Als(PathBuf),
}

#[derive(Debug)]
struct Args {
    source: Source,
    output: Option<PathBuf>,
    scene: Option<usize>,
    launcher: bool,
    sample_rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Render,
    Play,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(name @ ("render" | "play")) => {
            let command = if name == "render" { Command::Render } else { Command::Play };
            let result = parse_args(args.get(1..).unwrap_or_default(), command).and_then(|a| match command {
                Command::Render => render(&a),
                Command::Play => {
                    let (project, audio) = load_project(&a)?;
                    if a.launcher { play::launcher(project, &audio) } else { play::play(&project, &audio) }
                }
            });
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("error: {message}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("plugin") => match plugin(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("plugins") => match plugins(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("mcp") => match mcp(args.get(1..).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Some("--version" | "-V") => {
            println!("loopwerk-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_args(args: &[String], command: Command) -> Result<Args, String> {
    let mut output = None;
    let mut sample_rate = 48_000;
    let mut scene = None;
    let mut launcher = false;
    let mut source = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let next = match arg.as_str() {
            "--demo" => Some(Source::Demo),
            "-o" | "--output" => {
                output = Some(PathBuf::from(it.next().ok_or("--output needs a path")?));
                None
            }
            "--sample-rate" => {
                let value = it.next().ok_or("--sample-rate needs a value")?;
                sample_rate = value.parse().map_err(|_| format!("invalid sample rate: {value}"))?;
                None
            }
            "--scene" => {
                let value = it.next().ok_or("--scene needs a number")?;
                let n: usize = value.parse().map_err(|_| format!("invalid scene number: {value}"))?;
                scene = Some(n.checked_sub(1).ok_or("scenes are numbered from 1")?);
                None
            }
            "--launcher" => {
                launcher = true;
                None
            }
            other if other.starts_with('-') => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
            path => Some(Source::Als(PathBuf::from(path))),
        };
        if let Some(next) = next {
            if source.is_some() {
                return Err(format!("give one project, either a file or --demo\n\n{USAGE}"));
            }
            source = Some(next);
        }
    }
    match (command, &output) {
        (Command::Render, None) => return Err(format!("missing --output\n\n{USAGE}")),
        (Command::Play, Some(_)) => return Err(format!("play does not write a file; use render\n\n{USAGE}")),
        _ => {}
    }
    if launcher && (command != Command::Play || scene.is_some()) {
        return Err(format!("--launcher works with play, without --scene\n\n{USAGE}"));
    }
    Ok(Args {
        source: source.ok_or(format!("missing project: give a .als file or --demo\n\n{USAGE}"))?,
        output,
        scene,
        launcher,
        sample_rate,
    })
}

/// Opens the project named by the arguments and the audio files it uses, printing warnings, and
/// applies `--scene`.
fn load_project(args: &Args) -> Result<(Project, AudioPool), String> {
    let (project, audio) = match &args.source {
        Source::Demo => (demo_project(), AudioPool::new()),
        Source::Als(path) => {
            let import = loopwerk_als::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let folder = path.parent().unwrap_or(std::path::Path::new("."));
            let started = std::time::Instant::now();
            let (audio, audio_warnings) = loopwerk_media::load_project_audio(&import.project, folder);
            for warning in import.warnings.iter().chain(&audio_warnings) {
                eprintln!("warning: {warning}");
            }
            if !audio.files.is_empty() {
                eprintln!("audio: {} files decoded in {:.1} s", audio.files.len(), started.elapsed().as_secs_f64());
            }
            (import.project, audio)
        }
    };
    let project = match args.scene {
        Some(scene) => scene_as_arrangement(&project, scene)?,
        None => project,
    };
    Ok((project, audio))
}

fn render(args: &Args) -> Result<(), String> {
    let path = args.output.as_ref().ok_or("missing --output")?;
    let (project, audio) = load_project(args)?;
    let summary = render_wav(&project, &audio, path, args.sample_rate)?;
    println!("wrote {} ({:.2} s, {} Hz)", path.display(), summary.seconds, summary.sample_rate);
    Ok(())
}

/// Serves MCP on stdin/stdout. Nothing else may be printed to stdout while it runs.
/// `plugin run <plug-in> <in> <out> [--tail <seconds>]`.
fn plugin(args: &[String]) -> Result<(), String> {
    let mut words = args.iter().map(String::as_str);
    let (Some("run"), Some(name), Some(input), Some(output)) = (words.next(), words.next(), words.next(), words.next())
    else {
        return Err(format!(
            "usage: loopwerk-cli plugin run <plug-in> <in-audio> <out.wav> [--tail <seconds>]

{USAGE}"
        ));
    };
    let mut tail = 2.0f64;
    while let Some(word) = words.next() {
        match word {
            "--tail" => {
                tail = words
                    .next()
                    .and_then(|t| t.parse::<f64>().ok())
                    .filter(|t| t.is_finite() && (0.0..=600.0).contains(t))
                    .ok_or("--tail needs seconds, from 0 to 600")?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let mut library = loopwerk_plugins::library::Library::default();
    let spec = if ["clap:", "vst3:", "test:"].iter().any(|p| name.starts_with(p)) {
        name.to_string()
    } else {
        let found = library.catalog().find(name).ok_or_else(|| {
            format!("no plug-in {name:?} in the catalog; `loopwerk-cli plugins scan` looks for installed ones")
        })?;
        eprintln!("{} by {} ({})", found.name, found.vendor, found.path.display());
        found.spec()
    };

    let audio = loopwerk_media::decode_file(std::path::Path::new(input))?;
    let channels = usize::from(audio.channels.max(1));
    let stereo: Vec<f32> = audio
        .samples
        .chunks(channels)
        .flat_map(|frame| {
            let left = frame.first().copied().unwrap_or(0.0);
            [left, frame.get(1).copied().unwrap_or(left)]
        })
        .collect();
    let tail_frames = (tail * f64::from(audio.sample_rate)) as usize;
    let launch = loopwerk_plugins::Launch {
        host: library.host().to_path_buf(),
        plugin: spec,
        channels: 2,
        max_frames: loopwerk_plugins::offline::BLOCK_FRAMES,
        sample_rate: f64::from(audio.sample_rate),
        timeout: std::time::Duration::from_secs(30),
    };
    let started = std::time::Instant::now();
    let rendered = loopwerk_plugins::offline::render(launch, &stereo, tail_frames).map_err(|e| e.to_string())?;

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(output, spec).map_err(|e| format!("{output}: {e}"))?;
    for sample in &rendered.samples {
        writer.write_sample(*sample).map_err(|e| format!("{output}: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("{output}: {e}"))?;
    eprintln!(
        "wrote {output}: {:.1} s at {} Hz, plug-in latency {} frames (compensated), in {:.1} s",
        rendered.samples.len() as f64 / 2.0 / f64::from(audio.sample_rate),
        audio.sample_rate,
        rendered.latency,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// `plugins …`: runs the matching `plugins.*` command and prints its result.
fn plugins(args: &[String]) -> Result<(), String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let (id, params) = match words.as_slice() {
        ["scan"] => ("plugins.scan", serde_json::json!({})),
        ["scan", "--all"] => ("plugins.scan", serde_json::json!({ "all": true })),
        ["list"] => ("plugins.list", serde_json::json!({})),
        ["folders"] => ("plugins.folders", serde_json::json!({})),
        ["add-folder", folder] => ("plugins.addFolder", serde_json::json!({ "folder": folder })),
        ["remove-folder", folder] => ("plugins.removeFolder", serde_json::json!({ "folder": folder })),
        _ => {
            return Err(format!(
                "unknown plugins command

{USAGE}"
            ));
        }
    };
    if id == "plugins.scan" {
        eprintln!("scanning for plug-ins; each new or changed file is loaded in a process of its own");
    }
    let result = loopwerk_session::Projects::new().execute(id, &params).map_err(|e| e.0)?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

fn mcp(args: &[String]) -> Result<(), String> {
    let mut read_root = None;
    let mut write_root = None;
    let mut audio = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--read-root" => read_root = Some(PathBuf::from(it.next().ok_or("--read-root needs a folder")?)),
            "--write-root" => write_root = Some(PathBuf::from(it.next().ok_or("--write-root needs a folder")?)),
            "--audio" => audio = true,
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    let workspace = Workspace::new(read_root.as_deref(), write_root.as_deref())?;
    let mut server = LoopwerkMcp::new(workspace);
    if audio {
        server = server.with_audio_output(std::sync::Arc::new(play::DeviceOutput));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start the async runtime: {e}"))?;
    runtime.block_on(server.serve_stdio())
}

/// Two bars at 120 bpm: a bass line and a chord pattern on separate tracks, each placed twice.
fn demo_project() -> Project {
    let note = |time: f64, duration: f64, key: u8, velocity: f32| Note {
        time,
        duration,
        key,
        velocity,
        release_velocity: 0.5,
        channel: 0,
    };
    let track = |id: u32, name: &str| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        parent: None,
        extensions: Extensions::default(),
    };
    let channel = |id: u32, name: &str, volume: f64, pan: f64| Channel {
        id: ChannelId(id),
        name: name.into(),
        role: ChannelRole::Regular,
        volume,
        pan,
        mute: false,
        solo: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
    };
    let content = |id: u32, name: &str, notes: Vec<Note>| ClipContent {
        id: ContentId(id),
        name: name.into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
        extensions: Extensions::default(),
    };
    let clip = |content: u32, start: f64| Clip {
        content: ContentId(content),
        start,
        length: 4.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        extensions: Extensions::default(),
    };

    let bass = (0..8).map(|i| note(f64::from(i) * 0.5, 0.4, if i % 4 == 3 { 43 } else { 36 }, 0.9)).collect();
    let chords = [0.0, 1.5, 2.5].into_iter().flat_map(|t| [60, 64, 67].map(|k| note(t, 0.75, k, 0.5))).collect();

    Project {
        tracks: vec![track(1, "Bass"), track(2, "Chords")],
        channels: vec![channel(1, "Bass", 0.9, 0.0), channel(2, "Chords", 0.6, 0.3)],
        contents: vec![content(1, "Bass", bass), content(2, "Chords", chords)],
        arrangement: Arrangement {
            lanes: vec![
                ArrangementLane {
                    name: "Bass".into(),
                    track: Some(TrackId(1)),
                    clips: vec![clip(1, 0.0), clip(1, 4.0)],
                },
                ArrangementLane {
                    name: "Chords".into(),
                    track: Some(TrackId(2)),
                    clips: vec![clip(2, 0.0), clip(2, 4.0)],
                },
            ],
        },
        ..Project::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_project_is_valid_and_renders_two_bars() {
        let project = demo_project();
        assert_eq!(project.validate(), Ok(()));
        let samples = loopwerk_engine::render_offline(&project, 48_000, 512).unwrap();
        let seconds = samples.len() as f64 / 2.0 / 48_000.0;
        assert!((4.0..4.2).contains(&seconds), "{seconds} s");
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1 && peak <= 1.0, "peak {peak}");
    }

    #[test]
    fn render_args_need_one_project_and_an_output() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let parse_render = |a: &[String]| parse_args(a, Command::Render);
        assert!(parse_render(&args(&["-o", "x.wav"])).is_err());
        assert!(parse_render(&args(&["--demo"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o"])).is_err());
        assert!(parse_render(&args(&["--demo", "a.als", "-o", "x.wav"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--sample-rate", "abc"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--scene", "0"])).is_err());
        assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--bogus"])).is_err());
        let ok = parse_render(&args(&["a.als", "-o", "x.wav", "--sample-rate", "44100", "--scene", "2"])).unwrap();
        assert_eq!(ok.sample_rate, 44_100);
        assert_eq!(ok.scene, Some(1));
        assert!(matches!(ok.source, Source::Als(_)));
    }

    #[test]
    fn play_args_take_a_project_but_no_output() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse_args(&args(&["--demo"]), Command::Play).is_ok());
        assert!(parse_args(&args(&["a.als", "--scene", "3"]), Command::Play).is_ok());
        assert!(parse_args(&args(&["--demo", "-o", "x.wav"]), Command::Play).is_err());
        assert!(parse_args(&args(&[]), Command::Play).is_err());
        assert!(parse_args(&args(&["a.als", "--launcher"]), Command::Play).unwrap().launcher);
        assert!(parse_args(&args(&["a.als", "--launcher", "--scene", "1"]), Command::Play).is_err());
        assert!(parse_args(&args(&["a.als", "--launcher", "-o", "x.wav"]), Command::Render).is_err());
    }

    #[test]
    fn scene_replaces_the_arrangement_with_its_clips() {
        let mut project = demo_project();
        let clip = project.arrangement.lanes[0].clips[0].clone();
        project.launcher = Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot::default(), Slot { clip: Some(clip), has_stop: true }],
            }],
        };
        let scene_2 = scene_as_arrangement(&project, 1).unwrap();
        assert_eq!(scene_2.arrangement.lanes.len(), 1);
        assert_eq!(scene_2.arrangement.lanes[0].clips.len(), 1);
        assert!(scene_as_arrangement(&project, 0).unwrap().arrangement.lanes[0].clips.is_empty());
        assert!(scene_as_arrangement(&project, 2).is_err());
    }
}
