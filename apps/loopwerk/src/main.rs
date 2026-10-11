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
usage: loopwerk [<project.als> | --demo]

  <project.als>  a Live Set to import and open
  --demo         open a small built-in project
Without a project the window opens empty; drop a .als file on it to open one.";

enum Source {
    Nothing,
    Demo,
    Als(PathBuf),
}

fn main() -> ExitCode {
    let source = match parse_args(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(Some(source)) => source,
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
    match source {
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
        ..Default::default()
    };
    let result = eframe::run_native(
        "Loopwerk",
        options,
        Box::new(|cc| {
            loopwerk_ui::theme::install(&cc.egui_ctx);
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

fn parse_args(args: &[String]) -> Result<Option<Source>, String> {
    match args {
        [] => Ok(Some(Source::Nothing)),
        [flag] if flag == "--help" || flag == "-h" => Ok(None),
        [flag] if flag == "--demo" => Ok(Some(Source::Demo)),
        [flag] if flag.starts_with('-') => Err(format!("unknown argument: {flag}")),
        [path] => Ok(Some(Source::Als(PathBuf::from(path)))),
        _ => Err("give one project".into()),
    }
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

    #[test]
    fn one_project_or_none() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(matches!(parse_args(&args(&[])), Ok(Some(Source::Nothing))));
        assert!(matches!(parse_args(&args(&["--demo"])), Ok(Some(Source::Demo))));
        assert!(matches!(parse_args(&args(&["set.als"])), Ok(Some(Source::Als(_)))));
        assert!(matches!(parse_args(&args(&["--help"])), Ok(None)));
        assert!(parse_args(&args(&["--bogus"])).is_err());
        assert!(parse_args(&args(&["a.als", "b.als"])).is_err());
    }
}
