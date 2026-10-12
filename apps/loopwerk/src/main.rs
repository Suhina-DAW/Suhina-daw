//! Loopwerk, the desktop app: opens a project, plays it on the default audio output and shows it
//! in the main window (`loopwerk-ui`).

// Release builds on Windows start without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use loopwerk_engine::AudioPool;
use loopwerk_session::Session;
use loopwerk_ui::{App, Status};

const USAGE: &str = "\
usage: loopwerk [<project.als> | --demo] [--renderer wgpu|glow] [--no-animations]

  <project.als>      a Live Set to import and open
  --demo             open a small built-in project
  --renderer         how the window is drawn: wgpu (default) or glow (OpenGL)
  --no-animations    turn the interface's animations off
Without a project the window opens empty; drop a .als file on it to open one.";

#[derive(Debug, PartialEq)]
enum Source {
    Nothing,
    Demo,
    Als(PathBuf),
}

#[derive(Debug, PartialEq)]
struct Args {
    source: Source,
    renderer: eframe::Renderer,
    animations: bool,
}

fn main() -> ExitCode {
    let args = match parse_args(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut app = App::new();
    match args.source {
        Source::Nothing => {}
        Source::Demo => {
            let (session, warnings) = with_audio(Session::new(loopwerk_ui::demo::demo_project()), &AudioPool::new());
            app.set_session(session, &warnings);
        }
        Source::Als(path) => open(&mut app, &path),
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Loopwerk")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        renderer: args.renderer,
        ..Default::default()
    };
    let animations = args.animations;
    let result = eframe::run_native(
        "Loopwerk",
        options,
        Box::new(move |cc| {
            loopwerk_ui::theme::install(&cc.egui_ctx);
            App::set_animations(&cc.egui_ctx, animations);
            Ok(Box::new(Window { app }))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(None)` asks for the usage text.
fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    let mut parsed = Args { source: Source::Nothing, renderer: eframe::Renderer::Wgpu, animations: true };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let source = match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--demo" => Source::Demo,
            "--no-animations" => {
                parsed.animations = false;
                continue;
            }
            "--renderer" => {
                parsed.renderer = match it.next().map(String::as_str) {
                    Some("wgpu") => eframe::Renderer::Wgpu,
                    Some("glow") => eframe::Renderer::Glow,
                    other => return Err(format!("--renderer takes wgpu or glow, not {other:?}")),
                };
                continue;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown argument: {flag}")),
            path => Source::Als(PathBuf::from(path)),
        };
        if parsed.source != Source::Nothing {
            return Err("give one project".into());
        }
        parsed.source = source;
    }
    Ok(Some(parsed))
}

struct Window {
    app: App,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if let Some(path) = self.app.take_open_request() {
            open(&mut self.app, &path);
        }
    }
}

/// Opens a Live Set: imports it, decodes its audio and starts playing it, stopped.
fn open(app: &mut App, path: &Path) {
    match Session::open(path) {
        Ok((session, mut warnings)) => {
            let folder = path.parent().unwrap_or(Path::new("."));
            let (audio, audio_warnings) = loopwerk_media::load_project_audio(session.project(), folder);
            warnings.extend(audio_warnings);
            let (session, output_warnings) = with_audio(session, &audio);
            warnings.extend(output_warnings);
            app.set_session(session, &warnings);
        }
        Err(e) => app.set_status(Status::Error(e.0)),
    }
}

/// Connects the default audio output to `session`; without one the project still opens.
fn with_audio(mut session: Session, audio: &AudioPool) -> (Session, Vec<String>) {
    match loopwerk_output::open_player(session.project(), audio) {
        Ok((player, _device)) => {
            session.attach_player(player);
            (session, Vec::new())
        }
        Err(e) => (session, vec![format!("no audio output: {e}")]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: &[&str]) -> Result<Option<Args>, String> {
        parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn one_project_or_none() {
        assert_eq!(parse(&[]).ok().flatten().map(|a| a.source), Some(Source::Nothing));
        assert_eq!(parse(&["--demo"]).ok().flatten().map(|a| a.source), Some(Source::Demo));
        assert_eq!(parse(&["set.als"]).ok().flatten().map(|a| a.source), Some(Source::Als("set.als".into())));
        assert!(matches!(parse(&["--help"]), Ok(None)));
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["a.als", "b.als"]).is_err());
    }

    #[test]
    fn renderer_and_animations_can_be_chosen() {
        let args = parse(&["--demo", "--renderer", "glow", "--no-animations"]).ok().flatten();
        assert_eq!(args.as_ref().map(|a| a.renderer), Some(eframe::Renderer::Glow));
        assert_eq!(args.map(|a| a.animations), Some(false));
        assert!(parse(&["--renderer", "metal"]).is_err());
        assert!(parse(&["--renderer"]).is_err());
    }
}
