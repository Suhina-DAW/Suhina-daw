//! Loopwerk, the desktop app: opens a project, plays it on the default audio output and shows it
//! in the main window (`loopwerk-ui`).

// Release builds on Windows start without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use loopwerk_engine::AudioPool;
use loopwerk_session::{ProjectId, Session};
use loopwerk_ui::keymap::Keymap;
use loopwerk_ui::settings::{Graphics, Settings};
use loopwerk_ui::settings_view::About;
use loopwerk_ui::{App, OpenAs, Status};

const USAGE: &str = "\
usage: loopwerk [<project.als> | --demo] [--renderer wgpu|glow] [--no-animations]

  <project.als>      a Live Set to import and open
  --demo             open a small built-in project
  --renderer         how the window is drawn this time: wgpu or glow (OpenGL); the
                     default is the one chosen in Options > Settings
  --no-animations    turn the interface's animations off this time
Without a project the window opens empty; drop a .als file on it to open one.
Settings are kept in settings.txt and the keys in keymap.json, in Loopwerk's folder of the
user's app data.";

/// The folder eframe keeps Loopwerk's window state in; the settings file lives beside it.
const APP_ID: &str = "Loopwerk";
const SETTINGS_FILE: &str = "settings.txt";
const KEYMAP_FILE: &str = "keymap.json";

#[derive(Debug, PartialEq)]
enum Source {
    Nothing,
    Demo,
    Als(PathBuf),
}

#[derive(Debug, PartialEq)]
struct Args {
    source: Source,
    /// Chosen on the command line for this run, over the saved setting.
    renderer: Option<eframe::Renderer>,
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
    let (settings, mut settings_warnings) = load_settings();
    let (keymap, keymap_warnings) = load_keymap();
    settings_warnings.extend(keymap_warnings);
    let renderer = args.renderer.unwrap_or(match settings.graphics {
        Graphics::Gpu => eframe::Renderer::Wgpu,
        Graphics::OpenGl => eframe::Renderer::Glow,
    });
    let graphics_now = match renderer {
        eframe::Renderer::Glow => Graphics::OpenGl,
        eframe::Renderer::Wgpu => Graphics::Gpu,
    };
    let mut window = Window { app: App::new(), graphics_now, audio_output: None, pools: HashMap::new() };
    match args.source {
        Source::Nothing => {}
        Source::Demo => {
            let session = Session::new(loopwerk_ui::demo::demo_project());
            let (session, warnings, output) = with_audio(session, &AudioPool::new());
            window.audio_output = output;
            if let Some(id) = window.app.open_project(session, &warnings, OpenAs::NewTab) {
                window.pools.insert(id, AudioPool::new());
            }
        }
        Source::Als(path) => window.open(&path, OpenAs::NewTab),
    }
    if let Some(warning) = settings_warnings.first() {
        window.app.set_status(Status::Hint(warning.clone()));
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Loopwerk")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        renderer,
        ..Default::default()
    };
    let animations = args.animations;
    let result = eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            loopwerk_ui::theme::install(&cc.egui_ctx);
            window.app.set_settings(&cc.egui_ctx, settings);
            window.app.set_keymap(keymap);
            if !animations {
                App::set_animations(&cc.egui_ctx, false);
            }
            window.share_about();
            Ok(Box::new(window))
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
    let mut parsed = Args { source: Source::Nothing, renderer: None, animations: true };
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
                    Some("wgpu") => Some(eframe::Renderer::Wgpu),
                    Some("glow") => Some(eframe::Renderer::Glow),
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
    graphics_now: Graphics,
    audio_output: Option<String>,
    /// Each open project's decoded audio, kept so a project made active again can play.
    pools: HashMap<ProjectId, AudioPool>,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if let Some((path, how)) = self.app.take_open_request() {
            self.open(&path, how);
        }
        for closed in self.app.take_closed() {
            self.pools.remove(&closed);
        }
        // Only the active project plays; one made active again gets an output.
        if let Some(id) = self.app.take_player_request() {
            let opened = match (self.app.projects().get(id), self.pools.get(&id)) {
                (Some(session), Some(audio)) => Some(loopwerk_output::open_player(session.project(), audio)),
                _ => None,
            };
            match opened {
                Some(Ok((player, device))) => {
                    self.app.attach_player(id, player);
                    self.audio_output = Some(device);
                }
                Some(Err(e)) => self.app.set_status(Status::Error(format!("no audio output: {e}"))),
                None => {}
            }
            self.share_about();
        }
        if let Some(settings) = self.app.take_changed_settings()
            && let Err(e) = save_settings(&settings)
        {
            self.app.set_status(Status::Error(e));
        }
        if let Some(keymap) = self.app.take_changed_keymap()
            && let Err(e) = save_file(KEYMAP_FILE, &keymap.to_json())
        {
            self.app.set_status(Status::Error(e));
        }
    }
}

impl Window {
    /// Opens a Live Set in a new tab or in place of the active project: imports it, decodes its
    /// audio and starts playing it, stopped.
    fn open(&mut self, path: &Path, how: OpenAs) {
        match Session::open(path) {
            Ok((session, mut warnings)) => {
                let folder = path.parent().unwrap_or(Path::new("."));
                let (audio, audio_warnings) = loopwerk_media::load_project_audio(session.project(), folder);
                warnings.extend(audio_warnings);
                let (session, output_warnings, output) = with_audio(session, &audio);
                warnings.extend(output_warnings);
                self.audio_output = output;
                if let Some(id) = self.app.open_project(session, &warnings, how) {
                    self.app.set_waveforms(loopwerk_ui::waveform::from_pool(&audio));
                    self.pools.insert(id, audio);
                }
                for closed in self.app.take_closed() {
                    self.pools.remove(&closed);
                }
                self.share_about();
            }
            Err(e) => self.app.set_status(Status::Error(e.0)),
        }
    }

    /// Tells the Settings page what is in use.
    fn share_about(&mut self) {
        self.app.set_about(About { audio_output: self.audio_output.clone(), graphics_now: Some(self.graphics_now) });
    }
}

/// Connects the default audio output to `session`; without one the project still opens.
/// Returns the session, warnings and a description of the output.
fn with_audio(mut session: Session, audio: &AudioPool) -> (Session, Vec<String>, Option<String>) {
    match loopwerk_output::open_player(session.project(), audio) {
        Ok((player, device)) => {
            session.attach_player(player);
            (session, Vec::new(), Some(device))
        }
        Err(e) => (session, vec![format!("no audio output: {e}")], None),
    }
}

fn settings_path() -> Option<PathBuf> {
    eframe::storage_dir(APP_ID).map(|dir| dir.join(SETTINGS_FILE))
}

/// The saved settings, or the defaults when there are none yet.
fn load_settings() -> (Settings, Vec<String>) {
    match settings_path().map(std::fs::read_to_string) {
        Some(Ok(text)) => Settings::from_text(&text),
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
            (Settings::default(), vec![format!("could not read the settings: {e}")])
        }
        _ => (Settings::default(), Vec::new()),
    }
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    save_file(SETTINGS_FILE, &settings.to_text())
}

/// The saved keymap over the default keys, or the defaults when there is none yet.
fn load_keymap() -> (Keymap, Vec<String>) {
    let path = eframe::storage_dir(APP_ID).map(|dir| dir.join(KEYMAP_FILE));
    match path.map(std::fs::read_to_string) {
        Some(Ok(text)) => Keymap::from_json(&text),
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
            (Keymap::default(), vec![format!("could not read the keymap: {e}")])
        }
        _ => (Keymap::default(), Vec::new()),
    }
}

/// Writes one of Loopwerk's files in its app-data folder.
fn save_file(name: &str, text: &str) -> Result<(), String> {
    let dir = eframe::storage_dir(APP_ID).ok_or("there is no folder to keep settings in")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not save {name}: {e}"))?;
    std::fs::write(dir.join(name), text).map_err(|e| format!("could not save {name}: {e}"))
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
        assert_eq!(args.as_ref().map(|a| a.renderer), Some(Some(eframe::Renderer::Glow)));
        assert_eq!(args.map(|a| a.animations), Some(false));
        assert!(parse(&["--renderer", "metal"]).is_err());
        assert!(parse(&["--renderer"]).is_err());
    }
}
