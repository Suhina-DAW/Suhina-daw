//! Suhina, the desktop app: opens a project, plays it on the default audio output and shows it
//! in the main window (`suhina-ui`).

// Release builds on Windows start without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod recent;
mod theme_files;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use suhina_session::{OutputChoice, Session};
use suhina_ui::keymap::Keymap;
use suhina_ui::settings::{Graphics, Settings};
use suhina_ui::settings_view::About;
use suhina_ui::themes::FileAction;
use suhina_ui::{App, NewProject, OpenAs, Status};

const USAGE: &str = "\
usage: suhina [<project.locr> | <project.als> | --demo] [--renderer wgpu|glow] [--no-animations]

  <project.locr>     a Suhina project to open, or a Live Set (.als) to import
  --demo             open a small built-in project
  --renderer         how the window is drawn this time: wgpu or glow (OpenGL); the
                     default is the one chosen in Options > Settings
  --no-animations    turn the interface's animations off this time
Without a project the window opens empty; Ctrl+T makes a new one or opens one, and a
.locr or .als file dropped on it opens.
Settings are kept in settings.txt, the keys in keymap.json and themes in themes/<name>.json,
in Suhina's folder of the user's app data.";

/// The folder eframe keeps Suhina's window state in; the settings file lives beside it.
const APP_ID: &str = "Suhina";
const SETTINGS_FILE: &str = "settings.txt";
const KEYMAP_FILE: &str = "keymap.json";
const THEMES_FOLDER: &str = "themes";
const RECENT_FILE: &str = "recent.txt";

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
    let mut themes = theme_files::ThemeFiles::new(eframe::storage_dir(APP_ID).map(|d| d.join(THEMES_FOLDER)));
    let (theme_list, theme_warnings) = themes.read_all();
    settings_warnings.extend(theme_warnings);
    let recent = recent::Recent::load(eframe::storage_dir(APP_ID).map(|d| d.join(RECENT_FILE)));
    let mut window = Window { app: App::new(), graphics_now, editors_embeddable: false, themes, recent };
    // The audio devices, and the output the settings choose (Settings › Audio, `audio.*`).
    window.app.projects_mut().set_audio_system(std::sync::Arc::new(suhina_output::DeviceOutput::new()));
    window.app.projects_mut().set_output_choice(settings.output_choice());
    // The browser's library: browser.json and the folder indexes beside the settings (ADR 0012).
    let mut library = suhina_library::Library::new(eframe::storage_dir(APP_ID));
    settings_warnings.extend(library.take_warnings());
    if let Err(e) = library.start() {
        settings_warnings.push(e);
    }
    window.app.set_file_library(library);
    let (projects, templates) = default_folders();
    window.app.set_default_folders(projects, templates);
    match args.source {
        Source::Nothing => {}
        Source::Demo => {
            let session = Session::new(suhina_ui::demo::demo_project());
            let (session, warnings) = with_audio(session, &settings.output_choice());
            window.app.open_project(session, &warnings, OpenAs::NewTab);
        }
        Source::Als(path) => window.open(&path, OpenAs::NewTab),
    }
    if let Some(warning) = settings_warnings.first() {
        window.app.set_status(Status::Hint(warning.clone()));
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Suhina")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        renderer,
        event_loop_builder: x11_first(),
        ..Default::default()
    };
    let animations = args.animations;
    let result = eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            suhina_ui::theme::install(&cc.egui_ctx);
            let owner = native_window(cc);
            window.editors_embeddable = owner.is_some();
            window.app.set_editor_owner(owner);
            window.app.set_settings(&cc.egui_ctx, settings);
            let folder = window.themes.folder_text();
            window.app.set_themes(&cc.egui_ctx, theme_list, folder);
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
    /// The main window is a Win32 or X11 one, which plug-in editors can be embedded in.
    editors_embeddable: bool,
    themes: theme_files::ThemeFiles,
    /// The projects opened and saved lately, for the project dialog.
    recent: recent::Recent,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if let Some((path, how)) = self.app.take_open_request() {
            self.open(&path, how);
        }
        if let Some(new) = self.app.take_new_request() {
            self.new_project(new);
        }
        if let Some(path) = self.app.take_saved_path() {
            self.remember(&path);
        }
        if let Some(folder) = self.app.take_lists_wanted() {
            self.app.set_project_lists(self.recent.existing(), recent::templates(&folder));
        }
        // Each project keeps its decoded audio in its session (ADR 0011).
        let _closed = self.app.take_closed();
        // Only the active project plays; one made active again gets an output.
        if let Some(id) = self.app.take_player_request() {
            let choice = self.app.projects().output_choice().clone();
            let opened = self
                .app
                .projects()
                .get(id)
                .map(|session| suhina_output::open_chosen(session.project(), session.audio(), &choice, None));
            match opened {
                Some(Ok(opened)) => {
                    self.app.attach_player(id, opened.player);
                    if let Some(notice) = opened.notice {
                        self.app.set_status(Status::Hint(notice));
                    }
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
        self.sync_themes(ui.ctx());
    }
}

impl Window {
    /// Opens a Live Set in a new tab or in place of the active project: imports it, decodes its
    /// audio and starts playing it, stopped.
    fn open(&mut self, path: &Path, how: OpenAs) {
        match Session::open(path) {
            Ok((session, warnings)) => {
                self.show_session(session, warnings, how);
                self.remember(path);
            }
            Err(e) => self.app.set_status(Status::Error(e.0)),
        }
    }

    /// Makes a new project, blank or from a template, and opens it in a new tab.
    fn new_project(&mut self, new: NewProject) {
        let made = match new {
            NewProject::Blank => Ok((Session::new(suhina_session::blank_project()), Vec::new())),
            NewProject::Template(path) => Session::from_template(&path),
        };
        match made {
            Ok((session, warnings)) => self.show_session(session, warnings, OpenAs::NewTab),
            Err(e) => self.app.set_status(Status::Error(e.0)),
        }
    }

    /// Decodes a session's audio, starts it playing (stopped) and shows it.
    fn show_session(&mut self, mut session: Session, mut warnings: Vec<String>, how: OpenAs) {
        let folder = session.audio_folder().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let (audio, audio_warnings) = suhina_media::load_project_audio(session.project(), &folder);
        warnings.extend(audio_warnings);
        let waveforms = suhina_ui::waveform::from_pool(&audio);
        session.set_audio(audio);
        let choice = self.app.projects().output_choice().clone();
        let (session, output_warnings) = with_audio(session, &choice);
        warnings.extend(output_warnings);
        if self.app.open_project(session, &warnings, how).is_some() {
            self.app.set_waveforms(waveforms);
        }
        let _closed = self.app.take_closed();
        self.share_about();
    }

    /// Puts a project opened or saved first among the recent ones.
    fn remember(&mut self, path: &Path) {
        if let Err(e) = self.recent.add(path) {
            self.app.set_status(Status::Error(e));
        }
        self.app.record_project(path);
    }

    /// Does what Settings asked of the theme files (write, rename, read again, back up, export
    /// the template, bring in a dropped file, show the folder) and reads a theme file saved
    /// elsewhere. While a theme file is in use the folder is looked at every second.
    fn sync_themes(&mut self, ctx: &eframe::egui::Context) {
        let mut reload = false;
        for action in self.app.take_theme_actions() {
            let done = match action {
                FileAction::Save { id, theme } => self.themes.write(&id, &theme).map(|()| None),
                FileAction::Rename { from, to } => self.themes.rename(&from, &to).map(|()| None),
                FileAction::Reload => {
                    reload = true;
                    Ok(None)
                }
                FileAction::OpenFolder => self.themes.open_folder().map(|()| None),
                FileAction::Backup => self.themes.backup().map(|path| Some(saved(&path, "Backed up the themes to"))),
                FileAction::ExportTemplate => {
                    self.themes.export_template().map(|path| Some(saved(&path, "Saved the theme template to")))
                }
                FileAction::Import(path) => self.import_themes(ctx, &path).map(Some),
            };
            match done {
                Ok(Some(message)) => self.app.set_status(Status::Done(message)),
                Ok(None) => {}
                Err(e) => self.app.set_status(Status::Error(e)),
            }
        }
        let (entries, warnings) = if reload {
            let (entries, warnings) = self.themes.read_all();
            let count = entries.len();
            self.app.set_themes(ctx, entries, self.themes.folder_text());
            if warnings.is_empty() {
                self.app.set_status(Status::Done(format!("Read {count} theme files")));
            }
            (Vec::new(), warnings)
        } else {
            self.themes.changed()
        };
        for entry in entries {
            self.app.put_theme(ctx, entry);
        }
        if let Some(warning) = warnings.first() {
            self.app.set_status(Status::Error(warning.clone()));
        }
        if !suhina_ui::theme::is_built_in(&self.app.settings().theme) {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// Brings in a dropped theme file or zip and lists the themes again; a single new theme is
    /// put in use. Returns what to tell the user.
    fn import_themes(&mut self, ctx: &eframe::egui::Context, path: &Path) -> Result<String, String> {
        let (ids, warnings) = self.themes.import(path)?;
        let (entries, _) = self.themes.read_all();
        self.app.set_themes(ctx, entries, self.themes.folder_text());
        if let [id] = ids.as_slice() {
            self.app.use_theme(ctx, id);
        }
        let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let added = match ids.len() {
            0 => format!("{file} has no themes that are not here already"),
            1 => format!("Added the theme from {file}"),
            n => format!("Added {n} themes from {file}"),
        };
        Ok(match warnings.first() {
            Some(warning) => format!("{added}; {warning}"),
            None => added,
        })
    }

    /// Tells the Settings page what is in use.
    fn share_about(&mut self) {
        self.app.set_about(About {
            graphics_now: Some(self.graphics_now),
            editors_embeddable: self.editors_embeddable,
            ..Default::default()
        });
    }
}

/// "<what> <file>", and the file shown in the file manager.
fn saved(path: &Path, what: &str) -> String {
    let _shown = theme_files::reveal(path, true);
    format!("{what} {}", path.display())
}

/// Where projects are saved and templates kept until the settings say otherwise: `Suhina`
/// in the user's documents folder.
fn default_folders() -> (PathBuf, PathBuf) {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let documents = home.map(|h| h.join("Documents")).unwrap_or_default();
    let base = documents.join("Suhina");
    (base.join("Projects"), base.join("Templates"))
}

/// Connects the output `choice` names to `session`, playing its audio; a device that is gone
/// falls back to the default with a warning, and without any output the project still opens.
/// Returns the session and warnings.
fn with_audio(mut session: Session, choice: &OutputChoice) -> (Session, Vec<String>) {
    match suhina_output::open_chosen(session.project(), session.audio(), choice, None) {
        Ok(opened) => {
            session.attach_player(opened.player);
            (session, opened.notice.into_iter().collect())
        }
        Err(e) => (session, vec![format!("no audio output: {e}")]),
    }
}

/// The main window's native handle, for plug-in editors to float above it (ADR 0006): a Win32
/// `HWND` or an X11 window id; `None` elsewhere (Wayland, macOS: their editors float on their
/// own).
/// On Linux the window opens as an X11 one whenever an X server is there (`DISPLAY`): the
/// desktop's own, or XWayland on a Wayland desktop, which every distribution ships. Plug-in
/// editors are X11 windows (the hosts open them with `DISPLAY` too), and only an X11 window
/// can own them or embed them (ADR 0006); a Wayland window could do neither. Without
/// `DISPLAY` winit chooses as usual (Wayland), and editors open in windows of their own.
#[cfg(target_os = "linux")]
fn x11_first() -> Option<eframe::EventLoopBuilderHook> {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let display = std::env::var("DISPLAY").is_ok_and(|d| !d.is_empty());
    display.then(|| {
        Box::new(|builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
            builder.with_x11();
        }) as eframe::EventLoopBuilderHook
    })
}

#[cfg(not(target_os = "linux"))]
fn x11_first() -> Option<eframe::EventLoopBuilderHook> {
    None
}

// An X11 window id is a `c_ulong`: 32 bits on Windows, 64 on Linux, where `u64::from` changes
// nothing.
#[allow(clippy::useless_conversion)]
fn native_window(cc: &eframe::CreationContext<'_>) -> Option<u64> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => u64::try_from(handle.hwnd.get()).ok(),
        RawWindowHandle::Xlib(handle) => Some(u64::from(handle.window)),
        RawWindowHandle::Xcb(handle) => Some(u64::from(handle.window.get())),
        _ => None,
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

/// Writes one of Suhina's files in its app-data folder.
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
