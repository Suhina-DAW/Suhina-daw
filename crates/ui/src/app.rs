//! The main window: tabs for the open projects (ADR 0007), the control bar, the Session or
//! Arrangement view and the status bar around the active project.
//! Every change goes through `Session::execute` (AGENTS.md rule 6); the window only reads the
//! project and the engine's monitor to draw them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Align2, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::{CommandKind, ProjectId, Projects, Session};

use crate::arrangement_view::{self, ArrangementViewState};
use crate::browser::{BrowserState, ResultsMode};
use crate::browser_view;
use crate::detail_view;
use crate::drop::{self, BrowserItem, DropProbe, DropTarget};
use crate::grid::{Grid, SlotState};
use crate::keymap::Keymap;
use crate::loading_dialog;
use crate::motion;
use crate::project_dialog::{self, Choice, DialogState, Lists, SaveChoice, SavePrompt};
use crate::session_view::{self, SessionViewState};
use crate::settings::{DeviceLayout, EditorPlacement, OnOpen, Settings, Side};
use crate::settings_view::{self, About, PageState, PluginRequest, PluginsView, Sources};
use crate::theme::{self, PAD};
use crate::themes::{FileAction, ThemeEntry, Themes};
use crate::timeline::Timeline;
use crate::widgets::{self, Action, Drag};

/// Launch quantisation choices, as `launcher.setQuantization` takes them.
const QUANTIZATION: [&str; 14] = [
    "none", "8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32",
];
/// The engine's quantisation before anything is set.
const DEFAULT_QUANTIZATION: &str = "1 bar";

/// Height of the tab row, shown while two or more projects are open.
const TABS: f32 = 22.0;
/// The detail area's height in a row, by default, and the splitter between it and the view.
const DETAIL_HEIGHT: f32 = 200.0;
const SPLITTER: f32 = 6.0;

/// How dark the window gets while a project file is dragged over it.
const DROP_DIM: f32 = 0.45;

/// How a project the user asked to open is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAs {
    NewTab,
    /// In place of the active project, which is closed.
    Replace,
}

/// A new project the user asked for, for the app to make and open in a new tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewProject {
    Blank,
    /// An untitled project made from this template file.
    Template(PathBuf),
}

/// A finished plug-in scan: the library with its new catalog, and how many files were read and
/// failed, or why the scan could not run.
struct ScanDone {
    library: suhina_plugins::library::Library,
    report: Result<(usize, usize), String>,
}

/// The handle between the browser and the view.
const BROWSER_HANDLE: f32 = 4.0;

/// A drop whose samples are being decoded on another thread before its commands run.
struct PendingDrop {
    commands: Vec<(&'static str, serde_json::Value)>,
    /// Each decoded file (a Stems file once per audio track, with its stream number).
    decoded: std::sync::mpsc::Receiver<Vec<DecodedDrop>>,
}

type DecodedDrop = (PathBuf, Option<u16>, std::sync::Arc<suhina_engine::AudioData>, crate::waveform::Peaks);

/// A project's view state, kept while another project's tab is showing.
#[derive(Default)]
struct Stash {
    view: SessionViewState,
    arrangement: Option<ArrangementViewState>,
    quantization: Option<&'static str>,
    launches: HashMap<TrackId, (usize, f64)>,
    revision: u64,
    timeline: Option<(u64, Timeline)>,
    waveforms: std::sync::Arc<crate::waveform::Waveforms>,
}

/// The main view; Tab switches between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Session,
    Arrangement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Hint(String),
    Done(String),
    Error(String),
}

/// The window's state.
pub struct App {
    projects: Projects,
    /// The project whose view state is in the fields below; others are in `stashes`.
    shown_project: Option<ProjectId>,
    stashes: HashMap<ProjectId, Stash>,
    /// A dropped file waiting for the user to choose a new tab or replace.
    open_prompt: Option<PathBuf>,
    /// The active project's missing audio files, their bar and panel.
    missing: crate::missing_files::MissingFiles,
    remember_choice: bool,
    /// A project that became active and needs an audio output.
    player_request: Option<ProjectId>,
    /// Projects closed since the app last asked, so it can let go of their audio.
    closed: Vec<ProjectId>,
    shown: View,
    view: SessionViewState,
    arrangement: ArrangementViewState,
    status: Status,
    quantization: &'static str,
    /// Where each column's playing clip started, in beats, to show its progress.
    launches: HashMap<TrackId, (usize, f64)>,
    tempo_drag: Option<f64>,
    open_request: Option<(PathBuf, OpenAs)>,
    /// Counts changes to the project, so views derived from it are rebuilt only when it changed.
    revision: u64,
    /// The Arrangement view's data and the revision it was built from.
    timeline: Option<(u64, Timeline)>,
    /// Waveforms of the open project's audio files.
    waveforms: std::sync::Arc<crate::waveform::Waveforms>,
    /// The plug-in library as Settings shows it, and a scan running in the background.
    plugins: PluginsView,
    plugins_read: bool,
    scan: Option<std::sync::mpsc::Receiver<ScanDone>>,
    /// Which keys run which action, and whether that changed and should be saved.
    keymap: Keymap,
    keymap_changed: bool,
    /// The detail area is showing, and its height (in a row) or width (stacked).
    detail_open: bool,
    detail_height: f32,
    detail_width: f32,
    page: PageState,
    settings: Settings,
    /// The settings changed and should be saved.
    settings_changed: bool,
    /// The Settings page is showing instead of the view.
    settings_open: bool,
    /// The plug-in loading dialog was sent to the background for the project shown now.
    plugin_dialog_hidden: bool,
    about: About,
    /// The themes to pick from; the settings say which is in use.
    themes: Themes,
    /// The project dialog (File › New or open) while it shows, and what it lists.
    project_dialog: Option<DialogState>,
    project_lists: Lists,
    /// The dialog opened and its lists should be read again, by the app.
    lists_wanted: bool,
    /// The name and folder asked for when a project is saved under a new name or as a template.
    save_prompt: Option<SavePrompt>,
    new_request: Option<NewProject>,
    /// A project file saved since the app last asked, for its recent list.
    saved_path: Option<PathBuf>,
    /// Where projects are saved and templates kept when the settings leave them empty.
    default_folders: (PathBuf, PathBuf),
    /// The window's title as last set: the active project, marked when it has unsaved changes.
    window_title: String,
    /// The browser panel (`docs/spec/browser.md`), the same for every project.
    pub browser: BrowserState,
    /// The sample map, in the browser's results or large in the main area.
    pub sample_map: crate::sample_map_panel::Panel,
    pending_drop: Option<PendingDrop>,
    /// Where the detail area is this frame and the track it shows, for drops on its devices.
    detail_drop: Option<(Rect, TrackId)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            projects: Projects::new(),
            shown_project: None,
            stashes: HashMap::new(),
            open_prompt: None,
            missing: crate::missing_files::MissingFiles::default(),
            remember_choice: false,
            player_request: None,
            closed: Vec::new(),
            shown: View::Session,
            view: SessionViewState::default(),
            arrangement: ArrangementViewState::default(),
            status: Status::Hint("Drop a Live Set (.als) on the window to open it".into()),
            quantization: DEFAULT_QUANTIZATION,
            launches: HashMap::new(),
            tempo_drag: None,
            open_request: None,
            revision: 0,
            timeline: None,
            waveforms: std::sync::Arc::default(),
            plugins: PluginsView::default(),
            plugins_read: false,
            scan: None,
            keymap: Keymap::default(),
            keymap_changed: false,
            detail_open: true,
            detail_height: DETAIL_HEIGHT,
            detail_width: detail_view::STACK_WIDTH,
            page: PageState::default(),
            settings: Settings::default(),
            settings_changed: false,
            settings_open: false,
            plugin_dialog_hidden: false,
            about: About::default(),
            themes: Themes::default(),
            project_dialog: None,
            project_lists: Lists::default(),
            lists_wanted: false,
            save_prompt: None,
            new_request: None,
            saved_path: None,
            default_folders: (PathBuf::from("Projects"), PathBuf::from("Templates")),
            window_title: String::new(),
            browser: BrowserState::default(),
            sample_map: crate::sample_map_panel::Panel::default(),
            pending_drop: None,
            detail_drop: None,
        }
    }

    /// Uses `library` for the browser and the `library.*` commands (ADR 0012); the app gives
    /// it Suhina's settings folder and starts it.
    pub fn set_file_library(&mut self, library: suhina_library::Library) {
        self.projects.set_file_library(library);
        self.browser.relist();
    }

    /// The open projects, to change (the browser's library, for the app).
    pub fn projects_mut(&mut self) -> &mut Projects {
        &mut self.projects
    }

    /// Puts a project opened or saved first among the browser's recent projects.
    pub fn record_project(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Err(e) =
            self.projects.file_library_mut().record(suhina_library::Group::Projects, &path.display().to_string())
        {
            self.status = Status::Error(e);
        }
    }

    /// A window whose `plugins.*` commands use `library` instead of the one in Suhina's
    /// settings folder (tests, portable setups).
    pub fn with_library(library: suhina_plugins::library::Library) -> Self {
        Self { projects: Projects::with_library(library), ..Self::new() }
    }

    /// Shows `session`, replacing the active project. `warnings` are what the importer skipped.
    pub fn set_session(&mut self, session: Session, warnings: &[String]) {
        self.open_project(session, warnings, OpenAs::Replace);
    }

    /// Opens `session` in a new tab or in place of the active project, and makes it active.
    /// Returns its id, or `None` when no more projects can be open (the status bar says why).
    pub fn open_project(&mut self, session: Session, warnings: &[String], how: OpenAs) -> Option<ProjectId> {
        if how == OpenAs::Replace
            && let Some(active) = self.projects.active()
            && self.projects.close(active).is_ok()
        {
            self.stashes.remove(&active);
            self.closed.push(active);
            if self.shown_project == Some(active) {
                self.shown_project = None;
            }
        }
        self.stash_shown();
        let name = session
            .source()
            .and_then(|p| p.file_name())
            .map_or("the project".into(), |n| n.to_string_lossy().to_string());
        self.status = match warnings.len() {
            0 => Status::Done(format!("Opened {name}")),
            1 => Status::Hint(format!("Opened {name}: {}", warnings.first().map_or("", String::as_str))),
            n => Status::Hint(format!(
                "Opened {name} with {n} warnings; the first: {}",
                warnings.first().map_or("", String::as_str)
            )),
        };
        if session.player().is_none() {
            self.status = Status::Hint(format!("Opened {name} without audio output; launching needs one"));
        }
        let id = match self.projects.add(session, None) {
            Ok(id) => id,
            Err(e) => {
                self.status = Status::Error(e.0);
                return None;
            }
        };
        self.restore(id, Stash::default());
        self.plugin_dialog_hidden = false;
        Some(id)
    }

    /// Puts the shown project's view state aside, under its id.
    fn stash_shown(&mut self) {
        let Some(id) = self.shown_project.take() else { return };
        if self.projects.get(id).is_none() {
            return;
        }
        let stash = Stash {
            view: std::mem::take(&mut self.view),
            arrangement: Some(std::mem::take(&mut self.arrangement)),
            quantization: Some(self.quantization),
            launches: std::mem::take(&mut self.launches),
            revision: self.revision,
            timeline: self.timeline.take(),
            waveforms: std::mem::take(&mut self.waveforms),
        };
        self.stashes.insert(id, stash);
    }

    /// Shows project `id` with `stash` as its view state; the window's own preferences (the
    /// header column's side and width) carry over.
    fn restore(&mut self, id: ProjectId, stash: Stash) {
        let (headers_left, header_width) = (self.arrangement.headers_left, self.arrangement.header_width);
        self.view = stash.view;
        self.arrangement = stash.arrangement.unwrap_or_default();
        self.arrangement.headers_left = headers_left;
        self.arrangement.header_width = header_width;
        self.quantization = stash.quantization.unwrap_or(DEFAULT_QUANTIZATION);
        self.launches = stash.launches;
        self.revision = stash.revision.wrapping_add(1);
        self.timeline = stash.timeline;
        self.waveforms = stash.waveforms;
        self.shown_project = Some(id);
    }

    /// Follows a change of the active project (a tab switched or closed): swaps the view state
    /// and asks the app for an audio output when the newly active project has none.
    fn follow_active(&mut self) {
        let active = self.projects.active();
        if active == self.shown_project {
            return;
        }
        self.stash_shown();
        for gone in self.stashes.keys().copied().filter(|id| self.projects.get(*id).is_none()).collect::<Vec<_>>() {
            self.stashes.remove(&gone);
        }
        match active {
            Some(id) => {
                let stash = self.stashes.remove(&id).unwrap_or_default();
                self.restore(id, stash);
                if self.projects.get(id).is_some_and(|s| s.player().is_none()) {
                    self.player_request = Some(id);
                }
            }
            None => {
                self.shown_project = None;
                self.view = SessionViewState::default();
                self.timeline = None;
                self.launches.clear();
            }
        }
    }

    /// The window plug-in editors open over: the native handle of the app's main window (a
    /// Win32 `HWND`, an X11 window id), so they float above it (ADR 0006).
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.projects.set_editor_owner(owner);
    }

    /// The open projects.
    pub fn projects(&self) -> &Projects {
        &self.projects
    }

    /// A project that became active and needs an audio output, once.
    pub fn take_player_request(&mut self) -> Option<ProjectId> {
        self.player_request.take()
    }

    /// Projects closed since the last call, so the app can let go of their audio.
    pub fn take_closed(&mut self) -> Vec<ProjectId> {
        std::mem::take(&mut self.closed)
    }

    /// Lets project `id` play through `player`.
    pub fn attach_player(&mut self, id: ProjectId, player: suhina_session::Player) {
        if let Some(session) = self.projects.get_mut(id) {
            session.attach_player(player);
        }
        if self.shown_project == Some(id) {
            self.quantization = DEFAULT_QUANTIZATION;
        }
    }

    /// Asks to open `path`, as a dropped file does: at once when nothing is open or the
    /// settings say how, otherwise after the user chose in the window.
    pub fn request_open(&mut self, path: PathBuf) {
        if self.projects.is_empty() {
            self.open_request = Some((path, OpenAs::NewTab));
            return;
        }
        match self.settings.on_open {
            OnOpen::Ask => self.open_prompt = Some(path),
            OnOpen::NewTab => self.open_request = Some((path, OpenAs::NewTab)),
            OnOpen::Replace => self.open_request = Some((path, OpenAs::Replace)),
        }
    }

    /// The active project's session.
    pub fn session(&self) -> Option<&Session> {
        self.projects.active().and_then(|id| self.projects.get(id))
    }

    pub fn view(&self) -> View {
        self.shown
    }

    /// `transport.stop`, with where the playhead goes from the settings.
    fn stop_action(&self) -> Action {
        Action::new("transport.stop", json!({ "return": self.settings.stop.param() }))
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Turns animations on or off (a setting; on by default).
    pub fn set_animations(ctx: &egui::Context, on: bool) {
        motion::set_enabled(ctx, on);
    }

    /// Uses `settings` from now on, as loaded from the settings file.
    pub fn set_settings(&mut self, ctx: &egui::Context, settings: Settings) {
        motion::set_enabled(ctx, settings.animations);
        self.arrangement.headers_left = settings.track_headers == Side::Left;
        self.settings = settings;
        theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
    }

    /// Uses the themes read from the theme files in `folder`, and draws with the one the settings
    /// name (the built-in one when there is no such file).
    pub fn set_themes(&mut self, ctx: &egui::Context, files: Vec<ThemeEntry>, folder: Option<String>) {
        self.themes.set(files, folder);
        theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
    }

    /// Draws with the theme `id` from now on and saves that in the settings, if there is such a
    /// theme.
    pub fn use_theme(&mut self, ctx: &egui::Context, id: &str) {
        if self.themes.get(id).is_some() {
            self.settings.theme = id.to_string();
            self.settings_changed = true;
            theme::set_palette(ctx, self.themes.palette(id));
        }
    }

    /// Where projects are saved and templates kept when the settings leave them empty.
    pub fn set_default_folders(&mut self, projects: PathBuf, templates: PathBuf) {
        self.default_folders = (projects, templates);
    }

    /// The folder new projects are saved in: the setting, or the default.
    pub fn projects_folder(&self) -> PathBuf {
        match self.settings.projects_folder.trim() {
            "" => self.default_folders.0.clone(),
            folder => PathBuf::from(folder),
        }
    }

    /// The folder templates are kept in: the setting, or the default.
    pub fn templates_folder(&self) -> PathBuf {
        match self.settings.templates_folder.trim() {
            "" => self.default_folders.1.clone(),
            folder => PathBuf::from(folder),
        }
    }

    /// Shows the project dialog; `open` puts the keys in its path field (File › Open).
    pub fn open_project_dialog(&mut self, open: bool) {
        self.settings_open = false;
        self.save_prompt = None;
        self.project_dialog = Some(DialogState { path: String::new(), focus_path: open });
        self.lists_wanted = true;
    }

    pub fn project_dialog_open(&self) -> bool {
        self.project_dialog.is_some()
    }

    /// The templates folder to list, once each time the project dialog opens, for the app to
    /// read it and the recent projects and hand them back with [`App::set_project_lists`].
    pub fn take_lists_wanted(&mut self) -> Option<PathBuf> {
        std::mem::take(&mut self.lists_wanted).then(|| self.templates_folder())
    }

    /// The recent projects and the templates the project dialog lists.
    pub fn set_project_lists(&mut self, recent: Vec<PathBuf>, templates: Vec<PathBuf>) {
        self.project_lists =
            Lists { recent, templates, templates_folder: self.templates_folder().display().to_string() };
    }

    /// A new project the user asked for, for the app to make and open in a new tab.
    pub fn take_new_request(&mut self) -> Option<NewProject> {
        self.new_request.take()
    }

    /// A project file saved since the app last asked, for the recent projects.
    pub fn take_saved_path(&mut self) -> Option<PathBuf> {
        self.saved_path.take()
    }

    /// Saves the active project: to its own project file (`.suhina`, or the `.locr` it was
    /// opened from), or, the first time and when `ask`, to a name and folder the user gives.
    fn save_active(&mut self, ask: bool) {
        let Some(id) = self.projects.active() else { return };
        let Some(session) = self.projects.get(id) else { return };
        let own_file = session.source().filter(|s| suhina_session::is_project_file(s)).map(Path::to_path_buf);
        match own_file {
            Some(path) if !ask => self.save_to(&path, false),
            own => {
                let folder =
                    own.as_deref().and_then(Path::parent).map_or_else(|| self.projects_folder(), Path::to_path_buf);
                let name = self.projects.name(id).unwrap_or("Untitled").to_string();
                self.save_prompt =
                    Some(SavePrompt { template: false, name, folder: folder.display().to_string(), focus: true });
            }
        }
    }

    /// Asks for a name to save the active project as a template under.
    fn save_template(&mut self) {
        let Some(id) = self.projects.active() else { return };
        let name = self.projects.name(id).unwrap_or("Template").to_string();
        let folder = self.templates_folder().display().to_string();
        self.save_prompt = Some(SavePrompt { template: true, name, folder, focus: true });
    }

    /// Runs `project.save` on the active project; a template is a copy.
    fn save_to(&mut self, path: &Path, template: bool) {
        let params = json!({ "path": path.display().to_string(), "copy": template });
        match self.projects.execute("project.save", &params) {
            Ok(saved) => {
                let written = saved["path"].as_str().map_or_else(|| path.to_path_buf(), PathBuf::from);
                let what = if template { "Saved the template" } else { "Saved" };
                self.status = Status::Done(format!("{what} {}", written.display()));
                if !template {
                    self.saved_path = Some(written);
                }
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// The project dialog and the save prompt, over the window, and what was chosen in them.
    fn project_dialogs(&mut self, ui: &Ui, window: Rect) {
        let name = self.projects.active().and_then(|id| self.projects.name(id)).unwrap_or("the project").to_string();
        if let Some(crate::missing_files::Outcome::Apply(files)) = self.missing.panel(ui, window, &name) {
            self.relink(files);
        }
        if let Some(mut state) = self.project_dialog.take() {
            match project_dialog::show(ui, window, &mut state, &self.project_lists) {
                Some(Choice::Open(path)) => self.request_open(path),
                Some(Choice::NewBlank) => self.new_request = Some(NewProject::Blank),
                Some(Choice::NewFromTemplate(path)) => self.new_request = Some(NewProject::Template(path)),
                Some(Choice::Close) => {}
                None => self.project_dialog = Some(state),
            }
        }
        if let Some(mut prompt) = self.save_prompt.take() {
            match project_dialog::show_save(ui, window, &mut prompt) {
                Some(SaveChoice::Save(path)) => self.save_to(&path, prompt.template),
                Some(SaveChoice::Cancel) => {}
                None => self.save_prompt = Some(prompt),
            }
        }
    }

    /// The window's title: the active project, with a dot while it has unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.projects.active() {
            Some(id) => {
                let name = self.projects.name(id).unwrap_or("Untitled");
                let unsaved = self.projects.get(id).is_some_and(Session::has_unsaved_changes);
                format!("{name}{} - Suhina", if unsaved { " •" } else { "" })
            }
            None => "Suhina".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// A theme file read again after it changed on disk; the window redraws in it if it is the
    /// theme in use.
    pub fn put_theme(&mut self, ctx: &egui::Context, entry: ThemeEntry) {
        if self.themes.put(entry) {
            theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
        }
    }

    /// What to do with the theme files, in order, once: themes changed in Settings to write, a
    /// file to rename, the files to read again, a backup or the template to write, a dropped
    /// file to bring in.
    pub fn take_theme_actions(&mut self) -> Vec<FileAction> {
        self.themes.take_actions()
    }

    /// The waveforms of the open project's audio files, once they are decoded.
    pub fn set_waveforms(&mut self, waveforms: crate::waveform::Waveforms) {
        self.waveforms = std::sync::Arc::new(waveforms);
        self.timeline = None;
    }

    /// Uses `keymap` for the keys from now on, as loaded from the keymap file.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The keymap, once after each change, for the app to save.
    pub fn take_changed_keymap(&mut self) -> Option<Keymap> {
        std::mem::take(&mut self.keymap_changed).then(|| self.keymap.clone())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The settings, once after each change, for the app to save.
    pub fn take_changed_settings(&mut self) -> Option<Settings> {
        std::mem::take(&mut self.settings_changed).then(|| self.settings.clone())
    }

    /// Facts the Settings page shows, such as the audio output in use.
    pub fn set_about(&mut self, about: About) {
        self.about = about;
    }

    pub fn settings_open(&self) -> bool {
        self.settings_open
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// A project file the user asked to open, and how, for the app to open.
    pub fn take_open_request(&mut self) -> Option<(PathBuf, OpenAs)> {
        self.open_request.take()
    }

    /// Runs one command and shows its outcome in the status bar.
    pub fn run(&mut self, action: &Action) {
        if action.id.starts_with("projects.") {
            if let Err(e) = self.projects.execute(action.id, &action.params) {
                self.status = Status::Error(e.0);
            }
            return;
        }
        let Ok(session) = self.projects.session_mut(None) else { return };
        match session.execute(action.id, &action.params) {
            Ok(_) => {
                let changes_project = suhina_session::commands()
                    .iter()
                    .find(|c| c.id == action.id)
                    .is_some_and(|c| matches!(c.kind, CommandKind::Edit | CommandKind::History));
                if changes_project {
                    self.revision += 1;
                }
                if action.id == "launcher.setQuantization"
                    && let Some(value) = action.params["value"].as_str()
                    && let Some(known) = QUANTIZATION.iter().find(|q| **q == value)
                {
                    self.quantization = known;
                }
                if let Some(done) = describe(action) {
                    self.status = Status::Done(done);
                }
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Keeps the active project's plug-ins in line with it (ADR 0006) and shows what the user
    /// should hear about in the status bar. While the project has plug-ins, the window looks
    /// again a few times a second, so one that starts or crashes is noticed without input.
    fn poll_plugins(&mut self, ctx: &egui::Context) {
        self.projects.set_editor_embedding(self.settings.plugin_editors == EditorPlacement::DeviceView);
        let notices = self.projects.poll_plugins();
        if let Some(last) = notices.last() {
            self.status = Status::Hint(last.clone());
        }
        if self
            .session()
            .is_some_and(|s| s.player().is_some() && !suhina_engine::plugin_devices(s.project()).is_empty())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// While the active project's plug-ins start, a dialog over the window says how far they
    /// have got, until it is sent to the background or nothing is loading any more.
    fn plugin_dialog(&mut self, ui: &Ui, window: Rect) {
        if self.plugin_dialog_hidden || self.settings_open {
            return;
        }
        let Some(id) = self.projects.active() else { return };
        let Some(progress) = self.projects.get(id).map(Session::plugin_progress) else { return };
        if progress.loading.is_empty() {
            return;
        }
        let name = self.projects.name(id).unwrap_or("the project").to_string();
        if loading_dialog::show(ui, window, &name, &progress) {
            self.plugin_dialog_hidden = true;
        }
    }

    /// Draws the window into `ui` and runs what the user asked for.
    pub fn ui(&mut self, ui: &mut Ui) {
        let window = ui.max_rect();
        let mut actions = Vec::new();
        self.follow_active();
        self.poll_scan(ui.ctx());
        self.poll_plugins(ui.ctx());
        // The sample map's analysis goes on in the background; take in what it finished.
        self.projects.poll_sound_map();
        self.dropped_files(ui);
        keys_for_shortcuts(ui.ctx());
        self.shortcuts(ui, &mut actions);
        self.apply_zoom(ui.ctx());
        let scale = if self.settings.waveform_db { crate::waveform::Scale::Db } else { crate::waveform::Scale::Linear };
        crate::waveform::set_scale(ui.ctx(), scale);

        let grid = self.session().map(|s| Grid::build(s.project(), s.player().map(|p| p.monitor())));
        let progress = grid.as_ref().map(|g| self.progress(g)).unwrap_or_default();
        // The CPU load the control bar shows, from the engine's monitor, in per mille.
        let cpu = self.session().and_then(|s| s.player()).map(|p| p.cpu_monitor().load_milli());

        egui::Panel::top("menu-bar")
            .exact_size(theme::MENU_BAR)
            .frame(egui::Frame::NONE.fill(theme::panel_background()).inner_margin(egui::Margin::symmetric(4, 0)))
            .show(ui, |ui| self.menu_bar(ui, &mut actions));
        if self.projects.len() >= 2 {
            egui::Panel::top("tabs")
                .exact_size(TABS)
                .frame(egui::Frame::NONE.fill(theme::window_background()))
                .show(ui, |ui| self.tab_row(ui, &mut actions));
        }
        egui::Panel::top("control-bar")
            .exact_size(theme::CONTROL_BAR)
            .frame(egui::Frame::NONE.fill(theme::panel_background()))
            .show(ui, |ui| self.control_bar(ui, grid.as_ref(), cpu, &mut actions));
        egui::Panel::bottom("status-bar")
            .exact_size(theme::STATUS_BAR)
            .frame(egui::Frame::NONE.fill(theme::panel_background()))
            .show(ui, |ui| self.status_bar(ui, grid.as_ref()));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::border())).show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            // A 1 px gap shows the line colour between the bars and the view.
            let inner = Rect::from_min_max(area.min + Vec2::new(0.0, 1.0), area.max - Vec2::new(0.0, 1.0));
            if self.settings_open {
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| self.settings_page(ui));
                // The device panel is not shown: embedded editors hide with it.
                self.place_editors(&[]);
                return;
            }
            self.missing.sync(&mut self.projects, self.revision);
            let missing_bar = self.open_prompt.is_none() && self.missing.bar_shows();
            let prompt_height = if self.open_prompt.is_some() || missing_bar { theme::ROW + 12.0 } else { 0.0 };
            let (prompt, inner) = (
                Rect::from_min_size(inner.min, Vec2::new(inner.width(), prompt_height)),
                Rect::from_min_max(inner.min + Vec2::new(0.0, prompt_height), inner.max),
            );
            if self.open_prompt.is_some() {
                ui.scope_builder(egui::UiBuilder::new().max_rect(prompt), |ui| self.open_prompt_bar(ui));
            } else if missing_bar {
                ui.scope_builder(egui::UiBuilder::new().max_rect(prompt), |ui| self.missing.bar(ui, prompt));
            }
            let inner = if self.settings.browser_open { self.browser_panel(ui, inner) } else { inner };
            let view_area = inner;
            self.detail_drop = None;
            let inner = if grid.is_some() && self.detail_open {
                self.detail_area(ui, inner, &mut actions)
            } else {
                self.place_editors(&[]);
                inner
            };
            // A drag from the browser: the views say what is under the pointer.
            let probe = self
                .browser
                .drag
                .as_ref()
                .and_then(|items| ui.ctx().pointer_latest_pos().map(|pointer| DropProbe::new(pointer, items)));
            self.view.drop = probe.clone();
            self.arrangement.drop = probe;
            // The large sample map takes the main area, except while something is dragged from
            // it, so the drop can land on a track.
            if self.sample_map.view.large && self.browser.drag.is_none() {
                let (panel, browser, projects, settings) =
                    (&mut self.sample_map, &mut self.browser, &mut self.projects, &mut self.settings);
                if panel.show(ui, inner, browser, projects, settings, self.revision) {
                    self.settings_changed = true;
                }
            } else {
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| match (&grid, self.shown) {
                    (Some(grid), View::Session) => {
                        session_view::show(ui, grid, &progress, &mut self.view, &mut actions)
                    }
                    (Some(_), View::Arrangement) => {
                        if let Some(session) = self.projects.active().and_then(|id| self.projects.get(id)) {
                            self.arrangement.stack_audio = self.settings.sample_stacking;
                            self.arrangement.stack_midi = self.settings.midi_stacking;
                            let monitor = session.player().map(|p| p.monitor());
                            // Clips and their notes are rebuilt only after the project changed; the
                            // playhead is read every frame.
                            let timeline = match self.timeline.take() {
                                Some((revision, mut timeline)) if revision == self.revision => {
                                    timeline.follow(monitor);
                                    timeline
                                }
                                _ => {
                                    let mut timeline = Timeline::build(session.project(), monitor);
                                    timeline.waveforms = self.waveforms.clone();
                                    timeline
                                }
                            };
                            arrangement_view::show(ui, &timeline, &mut self.arrangement, &mut actions);
                            self.timeline = Some((self.revision, timeline));
                            // The headers can be moved from the view itself; that is the setting too.
                            let side = if self.arrangement.headers_left { Side::Left } else { Side::Right };
                            if side != self.settings.track_headers {
                                self.settings.track_headers = side;
                                self.settings_changed = true;
                            }
                        }
                    }
                    (None, _) => empty(ui),
                });
            }
            drop_overlay(ui, view_area);
        });
        self.browser_drag(ui);
        self.poll_drop(ui.ctx());

        self.plugin_dialog(ui, window);
        self.project_dialogs(ui, window);
        self.update_title(ui.ctx());
        for action in &actions {
            self.run(action);
        }
        if grid.is_some_and(|g| {
            g.running || g.columns.iter().any(|c| c.stop_queued || c.slots.iter().any(|s| s.state == SlotState::Queued))
        }) {
            ui.ctx().request_repaint();
        }
    }

    /// Lays out the detail area beside or under the view in `area`, draws it with its splitter,
    /// and returns what is left for the view.
    fn detail_area(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) -> Rect {
        let stacked = self.settings.device_layout == DeviceLayout::Stack;
        // What it shows: the selected clip, or the selected track's devices.
        let (track, clip) = match self.shown {
            View::Session => (self.view.selected, None),
            View::Arrangement => (self.arrangement.selected_tracks.first().copied(), self.arrangement.selected_clip()),
        };
        // Where each plug-in on the shown track stands, read first: reading it needs the session
        // itself, building the detail only the project; and an editor embedded in the panel
        // decides how tall the panel is.
        let mut reports = std::collections::HashMap::new();
        if let Some(shown_track) = clip.map(|(t, _)| t).or(track)
            && let Ok(session) = self.projects.session_mut(None)
        {
            let devices = session
                .project()
                .tracks
                .iter()
                .find(|t| t.id == shown_track)
                .and_then(|t| t.channel)
                .and_then(|c| session.project().channels.iter().find(|ch| ch.id == c))
                .map_or(0, |c| c.devices.len());
            for index in 0..devices {
                if let Some(report) = session.plugin_report(shown_track, index) {
                    reports.insert((shown_track, index), report);
                }
            }
        }
        if !stacked {
            let scale = ui.ctx().pixels_per_point();
            let needed = reports
                .values()
                .filter_map(|r| r.editor_embedded)
                .map(|(_, height)| detail_view::row_height_with_editor(height, scale))
                .fold(0.0, f32::max);
            self.detail_height = self.detail_height.max(needed);
        }
        let (view, splitter, detail) = if stacked {
            self.detail_width = self.detail_width.clamp(180.0, (area.width() - 240.0).max(180.0));
            let x = area.right() - self.detail_width;
            (
                Rect::from_min_max(area.min, Pos2::new(x - SPLITTER, area.bottom())),
                Rect::from_min_max(Pos2::new(x - SPLITTER, area.top()), Pos2::new(x, area.bottom())),
                Rect::from_min_max(Pos2::new(x, area.top()), area.max),
            )
        } else {
            self.detail_height = self.detail_height.clamp(80.0, (area.height() - 160.0).max(80.0));
            let y = area.bottom() - self.detail_height;
            (
                Rect::from_min_max(area.min, Pos2::new(area.right(), y - SPLITTER)),
                Rect::from_min_max(Pos2::new(area.left(), y - SPLITTER), Pos2::new(area.right(), y)),
                Rect::from_min_max(Pos2::new(area.left(), y), area.max),
            )
        };

        // The splitter: dragged to make the detail area bigger or smaller.
        let cursor = if stacked { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
        let response = ui.interact(splitter, Id::new("detail-splitter"), Sense::drag()).on_hover_cursor(cursor);
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            if stacked {
                self.detail_width -= delta.x;
            } else {
                self.detail_height -= delta.y;
            }
        }
        let painter = ui.painter_at(splitter);
        painter.rect_filled(splitter, 0.0, theme::control_background());
        let grip = if stacked { Vec2::new(2.0, 32.0) } else { Vec2::new(32.0, 2.0) };
        painter.rect_filled(Rect::from_center_size(splitter.center(), grip), 0.0, theme::text_hint());

        if let Some(shown_track) = clip.map(|(t, _)| t).or(track) {
            self.detail_drop = Some((detail, shown_track));
        }
        let shown = self.session().map_or(detail_view::Detail::Nothing, |s| {
            detail_view::build_with(s.project(), track, clip, &mut |t, i| reports.remove(&(t, i)))
        });
        let mut places = Vec::new();
        ui.scope_builder(egui::UiBuilder::new().max_rect(detail), |ui| {
            if stacked {
                detail_view::show_stack(ui, &shown, actions, &mut places);
            } else {
                detail_view::show_row(ui, &shown, actions, &mut places);
            }
        });
        self.place_editors(&places);
        view
    }

    /// Tells the active project where the panel shows its embedded plug-in editors this frame
    /// (none: they are hidden).
    fn place_editors(&mut self, places: &[detail_view::EditorPlace]) {
        let Ok(session) = self.projects.session_mut(None) else { return };
        let shown: Vec<_> = places.iter().map(|p| (p.track, p.device, p.placement)).collect();
        session.place_editors(&shown);
    }

    /// The tab row: one tab per open project, the active one lit; a tab's × closes it.
    fn tab_row(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let active = self.projects.active();
        let mut x = bar.left();
        for id in self.projects.ids() {
            let name = self.projects.name(id).unwrap_or("Untitled").to_string();
            // A dot after the name while the project has unsaved changes.
            let unsaved = self.projects.get(id).is_some_and(Session::has_unsaved_changes);
            let shown = if unsaved { format!("{name} •") } else { name.clone() };
            let galley = widgets::fitted(&painter, &shown, theme::ui_font(theme::TEXT), theme::text_primary(), 180.0);
            let width = galley.size().x + PAD * 2.0 + 16.0;
            let tab = Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(width, bar.height()));
            x = tab.right() + 1.0;
            let close = Rect::from_center_size(Pos2::new(tab.right() - PAD - 4.0, tab.center().y), Vec2::splat(12.0));
            let response = widgets::button_area(ui, tab, Id::new(("tab", id.0)), format!("Tab {name}"));
            let close_response = widgets::button_area(ui, close, Id::new(("close-tab", id.0)), format!("Close {name}"));
            if close_response.clicked() || response.middle_clicked() {
                actions.push(Action::new("projects.close", json!({ "project": id.0 })));
            } else if response.clicked() && Some(id) != active {
                actions.push(Action::new("projects.switch", json!({ "project": id.0 })));
            }
            let lit = motion::toward(ui.ctx(), response.id.with("active"), Some(id) == active, motion::VALUE);
            let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
            painter.rect_filled(tab, 0.0, motion::mix(idle, theme::panel_background(), lit));
            let text = motion::mix(theme::text_secondary(), theme::text_primary(), lit);
            painter.galley(Pos2::new(tab.left() + PAD, tab.center().y - galley.size().y / 2.0), galley, text);
            let cross = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &close_response));
            let c = close.center();
            for (a, b) in [(Vec2::new(-3.0, -3.0), Vec2::new(3.0, 3.0)), (Vec2::new(-3.0, 3.0), Vec2::new(3.0, -3.0))] {
                painter.line_segment([c + a, c + b], theme::hairline(cross));
            }
            painter.vline(tab.right() + 0.5, bar.y_range(), theme::hairline(theme::border()));
        }
    }

    /// The question asked when a project is dropped while another is open.
    fn open_prompt_bar(&mut self, ui: &mut Ui) {
        let Some(path) = self.open_prompt.clone() else { return };
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, theme::panel_background());
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        let current = self.projects.active().and_then(|id| self.projects.name(id)).unwrap_or("the project").to_string();
        let mut choice = None;
        let mut cancel = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(PAD, 6.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(format!("Open {name}:")).color(theme::text_primary()));
                if ui.button("In a new tab").clicked() {
                    choice = Some(OpenAs::NewTab);
                }
                if ui.button(format!("In place of {current}")).clicked() {
                    choice = Some(OpenAs::Replace);
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.checkbox(&mut self.remember_choice, "Remember");
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if let Some(how) = choice {
            if self.remember_choice {
                self.settings.on_open = match how {
                    OpenAs::NewTab => OnOpen::NewTab,
                    OpenAs::Replace => OnOpen::Replace,
                };
                self.settings_changed = true;
            }
            self.open_request = Some((path, how));
            self.open_prompt = None;
            self.remember_choice = false;
        } else if cancel {
            self.open_prompt = None;
            self.remember_choice = false;
        }
    }

    /// Reads the plug-in library through its commands, for the Settings page.
    fn read_plugins(&mut self) {
        self.plugins_read = true;
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let list = |v: &serde_json::Value| v.as_array().map(|a| a.iter().map(text).collect()).unwrap_or_default();
        match self.projects.execute("plugins.folders", &json!({})) {
            Ok(folders) => {
                self.plugins.standard = list(&folders["standard"]);
                self.plugins.added = list(&folders["added"]);
            }
            Err(e) => self.status = Status::Error(e.0),
        }
        match self.projects.execute("plugins.list", &json!({})) {
            Ok(found) => {
                let empty = Vec::new();
                self.plugins.found = found["plugins"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|p| [text(&p["name"]), text(&p["vendor"]), text(&p["format"]), text(&p["version"])])
                    .collect();
                self.plugins.failed = found["failed"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|f| (text(&f["file"]), text(&f["reason"])))
                    .collect();
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Does what the Settings page asked of the plug-in library. Folders change through their
    /// commands; a scan runs `Library::scan` (what `plugins.scan` runs) on a copy of the library
    /// on another thread, so the window keeps drawing, and the result replaces the library.
    fn plugin_request(&mut self, request: PluginRequest) {
        match request {
            PluginRequest::AddFolder(folder) => {
                match self.projects.execute("plugins.addFolder", &json!({ "folder": folder })) {
                    Ok(_) => self.plugins.new_folder.clear(),
                    Err(e) => self.status = Status::Error(e.0),
                }
                self.plugins_read = false;
            }
            PluginRequest::RemoveFolder(folder) => {
                if let Err(e) = self.projects.execute("plugins.removeFolder", &json!({ "folder": folder })) {
                    self.status = Status::Error(e.0);
                }
                self.plugins_read = false;
            }
            PluginRequest::Scan { all } => {
                let mut library = self.projects.library_mut().clone();
                let (done, receive) = std::sync::mpsc::channel();
                let started = std::thread::Builder::new().name("suhina-plugin-scan".into()).spawn(move || {
                    let report = library.scan(all).map(|r| (r.read.len(), r.failed.len()));
                    let _unheard = done.send(ScanDone { library, report });
                });
                match started {
                    Ok(_) => {
                        self.scan = Some(receive);
                        self.plugins.scanning = true;
                        self.status = Status::Hint("Scanning for plug-ins".into());
                    }
                    Err(e) => self.status = Status::Error(format!("could not start the scan: {e}")),
                }
            }
        }
    }

    /// Picks up a finished background scan.
    fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(receive) = &self.scan else { return };
        match receive.try_recv() {
            Ok(ScanDone { library, report }) => {
                *self.projects.library_mut() = library;
                self.status = match report {
                    Ok((read, failed)) => Status::Done(format!("Scan done: {read} files read, {failed} failed")),
                    Err(e) => Status::Error(e),
                };
                self.scan = None;
                self.plugins.scanning = false;
                self.plugins_read = false;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.status = Status::Error("the plug-in scan stopped without a result".into());
                self.scan = None;
                self.plugins.scanning = false;
            }
        }
    }

    /// Lists the audio hosts and devices for Settings › Audio (`audio.devices`).
    fn list_audio_devices(&mut self) {
        match self.projects.execute("audio.devices", &json!({})) {
            Ok(_) => {
                self.about.audio_hosts = self.projects.audio_system().map(|s| s.hosts()).unwrap_or_default();
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Plays on the output `choice` names from now on (`audio.setOutput`): the active project's
    /// output opens again there, the transport where it was.
    fn choose_output(&mut self, choice: &suhina_session::OutputChoice) {
        let params = choice.to_json();
        let params = json!({
            "host": params["host"].as_str().unwrap_or(""),
            "device": params["device"].as_str().unwrap_or(""),
            "sample_rate": choice.sample_rate.unwrap_or(0),
            "buffer_frames": choice.buffer_frames.unwrap_or(0),
            "exclusive": choice.exclusive,
        });
        match self.projects.execute("audio.setOutput", &params) {
            Ok(result) => {
                self.status = match result["notice"].as_str() {
                    Some(notice) => Status::Hint(notice.to_string()),
                    None => Status::Done(format!(
                        "Audio output: {}",
                        self.session()
                            .and_then(Session::player)
                            .and_then(suhina_session::Player::output_info)
                            .map_or_else(|| "chosen; it opens with the next project".to_string(), |i| i.describe())
                    )),
                };
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Whether a plug-in scan is running.
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// The Settings page, applying each change as it is made.
    fn settings_page(&mut self, ui: &mut Ui) {
        if !self.plugins_read {
            self.read_plugins();
        }
        let themes = self.themes.view(&self.settings.theme);
        self.about.projects_default = self.default_folders.0.display().to_string();
        self.about.templates_default = self.default_folders.1.display().to_string();
        let tag_limit = self.projects.file_library_mut().tag_limit();
        self.about.audio_live = self.session().and_then(Session::player).and_then(suhina_session::Player::output_info);
        if self.page.page == settings_view::Page::Audio {
            // The latency and underruns change while it plays.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        let sources = Sources {
            about: &self.about,
            plugins: &mut self.plugins,
            themes: &themes,
            keymap: &mut self.keymap,
            tag_limit,
        };
        let outcome = settings_view::show(ui, &mut self.settings, sources, &mut self.page);
        if let Some(request) = outcome.themes.clone() {
            if let Some(id) = self.themes.request(&self.settings.theme, request) {
                self.settings.theme = id;
                self.settings_changed = true;
            }
            theme::set_palette(ui.ctx(), self.themes.palette(&self.settings.theme));
        }
        if outcome.keymap_changed {
            self.keymap_changed = true;
        }
        if let Some(limit) = outcome.tag_limit
            && let Err(e) = self.projects.execute("library.setTag", &json!({ "limit": limit }))
        {
            self.status = Status::Error(e.0);
        }
        if let Some(request) = outcome.plugins.clone() {
            self.plugin_request(request);
        }
        if outcome.audio_refresh {
            self.list_audio_devices();
        }
        if let Some(choice) = outcome.audio_output.clone() {
            self.choose_output(&choice);
        }
        if outcome.changed {
            motion::set_enabled(ui.ctx(), self.settings.animations);
            self.arrangement.headers_left = self.settings.track_headers == Side::Left;
            self.settings_changed = true;
        }
        if outcome.close {
            self.settings_open = false;
        }
    }

    /// The menu bar: the same actions as the shortcuts and the views' controls.
    fn menu_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let has_project = self.session().is_some();
        let (can_undo, can_redo) = self.session().map_or((false, false), |s| (s.can_undo(), s.can_redo()));
        let several = self.projects.len() >= 2;
        let keymap = self.keymap.clone();
        let keys = move |action: &str| keymap.label(action).unwrap_or_default();
        ui.style_mut().visuals.widgets.inactive.fg_stroke = theme::hairline(theme::text_secondary());
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New or open…").shortcut_text(keys("file.new"))).clicked() {
                    self.open_project_dialog(false);
                }
                if ui.add(egui::Button::new("Open…").shortcut_text(keys("file.open"))).clicked() {
                    self.open_project_dialog(true);
                }
                ui.separator();
                if ui.add_enabled(has_project, egui::Button::new("Save").shortcut_text(keys("file.save"))).clicked() {
                    self.save_active(false);
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Save as…").shortcut_text(keys("file.saveAs")))
                    .clicked()
                {
                    self.save_active(true);
                }
                if ui
                    .add_enabled(
                        has_project,
                        egui::Button::new("Save as template…").shortcut_text(keys("file.saveTemplate")),
                    )
                    .clicked()
                {
                    self.save_template();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(can_undo, egui::Button::new("Undo").shortcut_text(keys("edit.undo"))).clicked() {
                    actions.push(Action::new("edit.undo", json!({})));
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text(keys("edit.redo"))).clicked() {
                    actions.push(Action::new("edit.redo", json!({})));
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add_enabled(has_project, egui::Button::new("Session").shortcut_text(keys("view.session")))
                    .clicked()
                {
                    self.shown = View::Session;
                    self.settings_open = false;
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Arrangement").shortcut_text(keys("view.arrangement")))
                    .clicked()
                {
                    self.shown = View::Arrangement;
                    self.settings_open = false;
                }
                let label = if self.sample_map.view.large { "Hide sample map" } else { "Sample map" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.sampleMap"))).clicked() {
                    self.settings_open = false;
                    self.toggle_large_map();
                }
                let missing = self.missing.rows.len();
                let label =
                    if missing == 0 { "Missing files…".to_string() } else { format!("Missing files ({missing})…") };
                if ui.add_enabled(missing > 0, egui::Button::new(label)).clicked() {
                    self.missing.open = true;
                    self.settings_open = false;
                }
                let label = if self.settings.browser_open { "Hide browser" } else { "Show browser" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.browser"))).clicked() {
                    self.toggle_browser();
                }
                let label = if self.detail_open { "Hide detail area" } else { "Show detail area" };
                if ui.add_enabled(has_project, egui::Button::new(label).shortcut_text(keys("view.detail"))).clicked() {
                    self.detail_open = !self.detail_open;
                }
                ui.separator();
                let (zoom, limits) = (self.settings.zoom, crate::settings::ZOOMS);
                if ui
                    .add_enabled(zoom < limits.1, egui::Button::new("Zoom in").shortcut_text(keys("view.zoomIn")))
                    .clicked()
                {
                    self.step_zoom(1);
                }
                if ui
                    .add_enabled(zoom > limits.0, egui::Button::new("Zoom out").shortcut_text(keys("view.zoomOut")))
                    .clicked()
                {
                    self.step_zoom(-1);
                }
                let reset = egui::Button::new(format!("Actual size ({zoom} %)")).shortcut_text(keys("view.zoomReset"));
                if ui.add_enabled(zoom != crate::settings::ZOOM, reset).clicked() {
                    self.settings.zoom = crate::settings::ZOOM;
                    self.settings_changed = true;
                }
            });
            ui.menu_button("Projects", |ui| {
                if ui
                    .add_enabled(several, egui::Button::new("Next project").shortcut_text(keys("projects.next")))
                    .clicked()
                {
                    actions.push(Action::new("projects.next", json!({})));
                }
                if ui
                    .add_enabled(
                        several,
                        egui::Button::new("Previous project").shortcut_text(keys("projects.previous")),
                    )
                    .clicked()
                {
                    actions.push(Action::new("projects.previous", json!({})));
                }
                if let Some(active) = self.projects.active()
                    && ui.button("Close project").clicked()
                {
                    actions.push(Action::new("projects.close", json!({ "project": active.0 })));
                }
            });
            ui.menu_button("Options", |ui| {
                if ui.add(egui::Button::new("Settings").shortcut_text(keys("settings.open"))).clicked() {
                    self.settings_open = true;
                }
            });
        });
    }

    fn dropped_files(&mut self, ui: &Ui) {
        // The missing files panel takes a dropped folder as the folder to search.
        if self.missing.open {
            return;
        }
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        let is = |path: &PathBuf, ext: &str| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
        let mut opened = false;
        for path in dropped {
            if path.is_dir() {
                match self.projects.execute("library.addFolder", &json!({ "folder": path.display().to_string() })) {
                    Ok(_) => {
                        self.settings.browser_open = true;
                        self.browser.relist();
                        self.status = Status::Done(format!("Added {} to the browser", path.display()));
                    }
                    Err(e) => self.status = Status::Error(e.0),
                }
            } else if is(&path, "json") || is(&path, "zip") {
                self.themes.import(path);
            } else if (is(&path, "als") || suhina_session::is_project_file(&path)) && !opened {
                opened = true;
                self.request_open(path);
            } else {
                let what = "is not a project (.suhina), a Live Set (.als), a theme (.json), a zip of themes or a folder for the browser";
                self.status = Status::Error(format!("{} {what}", path.display()));
            }
        }
    }

    /// Runs the actions whose keys were pressed (the keymap), unless a text field has the
    /// keyboard or Settings is waiting for a key to bind.
    fn shortcuts(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        let dialog = self.project_dialog.is_some() || self.save_prompt.is_some();
        if ui.ctx().egui_wants_keyboard_input() || dialog || (self.settings_open && self.page.capture.is_some()) {
            return;
        }
        let pressed = ui.input_mut(|i| self.keymap.pressed(i));
        for action in pressed {
            self.do_action(&action, actions);
        }
    }

    /// Does one keymap action: the window's own, one on the selection, or a command that needs
    /// no parameters.
    pub fn do_action(&mut self, action: &str, actions: &mut Vec<Action>) {
        let has_project = !self.projects.is_empty();
        match action {
            "settings.open" => self.settings_open = !self.settings_open,
            "file.new" => self.open_project_dialog(false),
            "file.open" => self.open_project_dialog(true),
            "view.browser" => self.toggle_browser(),
            "view.sampleMap" => {
                self.settings_open = false;
                self.toggle_large_map();
            }
            "view.zoomIn" => self.step_zoom(1),
            "view.zoomOut" => self.step_zoom(-1),
            "view.zoomReset" => {
                if self.settings.zoom != crate::settings::ZOOM {
                    self.settings.zoom = crate::settings::ZOOM;
                    self.settings_changed = true;
                }
            }
            "browser.search" => {
                if !self.settings.browser_open {
                    self.toggle_browser();
                }
                self.settings_open = false;
                self.browser.focus_search = true;
            }
            _ if !has_project => {}
            "file.save" => self.save_active(false),
            "file.saveAs" => self.save_active(true),
            "file.saveTemplate" => self.save_template(),
            "view.toggle" => {
                self.shown = match self.shown {
                    View::Session => View::Arrangement,
                    View::Arrangement => View::Session,
                };
            }
            "view.session" => {
                self.shown = View::Session;
                self.close_large_map();
            }
            "view.detail" => self.detail_open = !self.detail_open,
            "view.arrangement" => {
                self.shown = View::Arrangement;
                self.close_large_map();
            }
            "transport.toggle" => {
                let running = self.session().and_then(|s| s.player()).is_some_and(|p| p.monitor().running());
                actions.push(if running { self.stop_action() } else { Action::new("transport.play", json!({})) });
            }
            "selection.copy" | "selection.cut" | "selection.duplicate" | "selection.delete" => {
                let Some((track, index)) = self.arrangement.selected_clip() else { return };
                let id = match action {
                    "selection.copy" => "clip.copy",
                    "selection.cut" => "clip.cut",
                    "selection.duplicate" => "clip.duplicate",
                    _ => "clip.delete",
                };
                if matches!(id, "clip.cut" | "clip.delete") {
                    self.arrangement.clear_selected_clip();
                }
                let mut params = json!({ "track": track.0, "index": index });
                // A copy follows the stacking setting for its kind, as a moved clip does.
                if id == "clip.duplicate" {
                    let audio = self
                        .timeline
                        .as_ref()
                        .and_then(|(_, t)| t.lanes.iter().find(|l| l.track == track))
                        .and_then(|l| l.clips.iter().find(|c| c.index == index))
                        .map(|c| c.audio);
                    if audio.is_some_and(|audio| self.stacks(audio)) {
                        params["stack"] = json!(true);
                    }
                }
                actions.push(Action::new(id, params));
            }
            "selection.paste" => {
                // After the selected clip, or at the playhead on the selected track.
                let timeline = self.timeline.as_ref().map(|(_, t)| t);
                let target = match self.arrangement.selected_clip() {
                    Some((track, index)) => timeline
                        .and_then(|t| t.lanes.iter().find(|l| l.track == track))
                        .and_then(|l| l.clips.iter().find(|c| c.index == index))
                        .map(|c| (track, c.start + c.length)),
                    None => self.arrangement.selected_tracks.first().map(|t| (*t, timeline.map_or(0.0, |t| t.beat))),
                };
                match target {
                    Some((track, start)) => {
                        let mut params = json!({ "track": track.0, "start": start });
                        // The pasted clip follows the stacking setting for its kind, as a moved clip does.
                        let audio = self.session().and_then(Session::copied_clip_is_audio);
                        if audio.is_some_and(|audio| self.stacks(audio)) {
                            params["stack"] = json!(true);
                        }
                        actions.push(Action::new("clip.paste", params))
                    }
                    None => self.status = Status::Hint("Select a clip or a track to paste on".into()),
                }
            }
            "selection.group" => {
                if self.arrangement.selected_tracks.is_empty() {
                    return;
                }
                let tracks: Vec<u32> = self.arrangement.selected_tracks.iter().map(|t| t.0).collect();
                actions.push(Action::new("track.group", json!({ "tracks": tracks })));
                self.arrangement.selected_tracks.clear();
            }
            "projects.next" | "projects.previous" if self.projects.len() < 2 => {}
            command => actions.push(Action::new_owned(command, json!({}))),
        }
    }

    /// Whether a clip of this kind (audio, or notes) leaves the clips it lands on as they are:
    /// Options › Settings › sample stacking for audio, MIDI stacking for notes.
    fn stacks(&self, audio: bool) -> bool {
        if audio { self.settings.sample_stacking } else { self.settings.midi_stacking }
    }

    /// Makes the whole window `steps` zoom steps larger or smaller and keeps that in the settings.
    fn step_zoom(&mut self, steps: i32) {
        if self.settings.step_zoom(steps) {
            self.settings_changed = true;
        }
    }

    /// Draws the window at the zoom the settings keep. The keymap owns the zoom keys, so the
    /// toolkit's own Ctrl and +, which the settings would not remember, is turned off.
    fn apply_zoom(&self, ctx: &egui::Context) {
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        let wanted = self.settings.zoom_factor();
        if (ctx.zoom_factor() - wanted).abs() > f32::EPSILON {
            ctx.set_zoom_factor(wanted);
        }
    }

    /// Shows or hides the browser panel, as Ctrl+B does.
    pub fn set_browser_open(&mut self, open: bool) {
        if self.settings.browser_open != open {
            self.toggle_browser();
        }
    }

    fn toggle_browser(&mut self) {
        self.settings.browser_open = !self.settings.browser_open;
        self.settings_changed = true;
        if !self.settings.browser_open {
            browser_view::stop_preview(&mut self.browser, &mut self.projects);
            self.browser.drag = None;
        }
    }

    /// The track selected in the view shown.
    fn selected_track(&self) -> Option<TrackId> {
        match self.shown {
            View::Session => self.view.selected,
            View::Arrangement => self.arrangement.selected_tracks.first().copied(),
        }
    }

    /// Lays out the browser at the side of `area`, draws it with its handle, and returns what is
    /// left for the view.
    fn browser_panel(&mut self, ui: &mut Ui, area: Rect) -> Rect {
        let (low, high) = crate::settings::BROWSER_WIDTHS;
        let width = (self.settings.browser_width as f32)
            .clamp(low as f32, (high as f32).min(area.width() - 240.0).max(low as f32));
        let left = self.settings.browser_side == Side::Left;
        let (panel, handle, rest) = if left {
            let panel = Rect::from_min_max(area.min, Pos2::new(area.left() + width, area.bottom()));
            let handle = Rect::from_min_max(
                Pos2::new(panel.right(), area.top()),
                Pos2::new(panel.right() + BROWSER_HANDLE, area.bottom()),
            );
            (panel, handle, Rect::from_min_max(Pos2::new(handle.right(), area.top()), area.max))
        } else {
            let panel = Rect::from_min_max(Pos2::new(area.right() - width, area.top()), area.max);
            let handle = Rect::from_min_max(
                Pos2::new(panel.left() - BROWSER_HANDLE, area.top()),
                Pos2::new(panel.left(), area.bottom()),
            );
            (panel, handle, Rect::from_min_max(area.min, Pos2::new(handle.left(), area.bottom())))
        };
        if let Some(new) = browser_view::resize_handle(ui, handle, width, left) {
            let new = new.round().clamp(low as f32, high as f32) as u32;
            if new != self.settings.browser_width {
                self.settings.browser_width = new;
                self.settings_changed = true;
            }
        }
        let search_key = self.keymap.label("browser.search").unwrap_or_default();
        let selected_track = self.selected_track();
        let revision = self.revision;
        let mut map_settings = self.settings.clone();
        let map_panel = &mut self.sample_map;
        let mut draw_map = |ui: &mut Ui, rect: Rect, state: &mut BrowserState, projects: &mut Projects| {
            map_panel.show(ui, rect, state, projects, &mut map_settings, revision);
        };
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                let cx = browser_view::Context {
                    projects: &mut self.projects,
                    settings: &self.settings,
                    selected_track,
                    search_key,
                    other_results: Some(&mut draw_map),
                };
                browser_view::show(ui, panel, &mut self.browser, cx)
            })
            .inner;
        if map_settings != self.settings {
            self.settings = map_settings;
            self.settings_changed = true;
        }
        if let Some(why) = self.sample_map.status.take() {
            self.status = Status::Hint(why);
        }
        let added = std::mem::take(&mut self.sample_map.add);
        if !added.is_empty() {
            self.add_to_selected(added);
        }
        if let Some(status) = out.status {
            self.status = status;
        }
        if let Some(path) = out.open_project {
            self.request_open(path);
        }
        if !out.add.is_empty() {
            self.add_to_selected(out.add);
        }
        rest
    }

    /// Shows the sample map large in the main area, or closes it back into the browser.
    fn toggle_large_map(&mut self) {
        let large = !self.sample_map.view.large;
        self.sample_map.view.large = large;
        self.browser.mode = if large { ResultsMode::List } else { ResultsMode::Other };
    }

    fn close_large_map(&mut self) {
        if self.sample_map.view.large {
            self.toggle_large_map();
        }
    }

    /// Adds browser items to the selected track, as Enter and a double-click do: devices to its
    /// chain, files in the Arrangement at the play start marker, in the Session from its first
    /// empty slot; without a selected track, on new tracks.
    fn add_to_selected(&mut self, items: Vec<BrowserItem>) {
        let Some(session) = self.session() else {
            self.status = Status::Hint("Open a project to add to".into());
            return;
        };
        let project = session.project();
        let track = self.selected_track().filter(|t| project.tracks.iter().any(|x| x.id == *t));
        let files = items.iter().any(|i| i.kind.is_file_clip());
        let target = match (track, self.shown) {
            (Some(track), _) if !files => DropTarget::Track(track),
            (Some(track), View::Arrangement) => {
                let beat = session.player().map_or(0.0, |p| p.monitor().start_beat());
                DropTarget::Lane { track, beat }
            }
            (Some(track), View::Session) => {
                let column = project.launcher.columns.iter().find(|c| c.track == track);
                let scene = column
                    .and_then(|c| c.slots.iter().position(|s| s.clip.is_none()))
                    .unwrap_or(project.launcher.scenes.len());
                DropTarget::Slot { track, scene }
            }
            (None, View::Arrangement) => DropTarget::NewTracks { beat: Some(0.0), scene: None },
            (None, View::Session) => DropTarget::NewTracks { beat: None, scene: Some(0) },
        };
        self.drop_items(&items, target, false);
    }

    /// While items are dragged from the browser: the label beside the pointer, Escape to stop,
    /// and the drop when the button is let go.
    fn browser_drag(&mut self, ui: &Ui) {
        let Some(items) = self.browser.drag.clone() else { return };
        let (released, escape, ctrl, pointer) = ui.input(|i| {
            (!i.pointer.primary_down(), i.key_pressed(Key::Escape), i.modifiers.command, i.pointer.latest_pos())
        });
        let mut target = match self.shown {
            View::Session => self.view.drop.as_ref().and_then(|p| p.target),
            View::Arrangement => self.arrangement.drop.as_ref().and_then(|p| p.target),
        };
        if let (None, Some(pointer), Some((detail, track))) = (target, pointer, self.detail_drop)
            && detail.contains(pointer)
            && items.iter().any(|i| i.kind.is_device())
        {
            target = Some(DropTarget::Track(track));
        }
        if escape {
            self.browser.drag = None;
            return;
        }
        let separate = ctrl != self.settings.drop_separate;
        let label = target.and_then(|t| self.plan_drop(&items, t, separate)).map(|p| p.label);
        if let Some(pointer) = pointer {
            let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("browser-drag"));
            let painter = ui.ctx().layer_painter(layer);
            let text = label.clone().unwrap_or_else(|| match items.as_slice() {
                [one] => one.name.clone(),
                many => format!("{} items", many.len()),
            });
            let font = theme::ui_font(theme::TEXT_SMALL);
            let galley = widgets::fitted(&painter, &text, font, theme::text_primary(), 280.0);
            let rect = Rect::from_min_size(pointer + Vec2::new(14.0, 10.0), galley.size() + Vec2::new(PAD * 2.0, 6.0));
            painter.rect_filled(rect, 0.0, theme::control_active());
            painter.rect_stroke(
                rect,
                0.0,
                theme::hairline(if label.is_some() { theme::accent() } else { theme::border_strong() }),
                egui::StrokeKind::Inside,
            );
            painter.galley(rect.min + Vec2::new(PAD, 3.0), galley, theme::text_primary());
            ui.ctx().request_repaint();
        }
        if released {
            self.browser.drag = None;
            self.view.drop = None;
            self.arrangement.drop = None;
            if let Some(target) = target {
                // Dropped into the project: the preview has done its job.
                browser_view::stop_preview(&mut self.browser, &mut self.projects);
                self.drop_items(&items, target, separate);
            }
        }
    }

    fn plan_drop(&self, items: &[BrowserItem], target: DropTarget, separate: bool) -> Option<drop::DropPlan> {
        let project = self.session()?.project();
        let name =
            |t: TrackId| project.tracks.iter().find(|x| x.id == t).map_or_else(|| t.0.to_string(), |x| x.name.clone());
        let stack = self.settings.sample_stacking || self.settings.midi_stacking;
        drop::plan(items, target, separate, self.settings.warp_mode.param(), stack, &name)
    }

    /// Runs a drop: decodes its samples on another thread first, so the window never waits,
    /// then runs its commands (ADR 0011).
    fn drop_items(&mut self, items: &[BrowserItem], target: DropTarget, separate: bool) {
        let Some(plan) = self.plan_drop(items, target, separate) else {
            self.status = Status::Hint("Nothing to add there".into());
            return;
        };
        self.decode_then_run(plan.samples, plan.commands);
    }

    /// Relinks missing files (`project.relinkFiles`), their audio decoded on another thread first.
    fn relink(&mut self, files: Vec<(String, PathBuf)>) {
        let names: Vec<String> = files.iter().map(|(f, _)| f.clone()).collect();
        let to: Vec<String> = files.iter().map(|(_, p)| p.display().to_string()).collect();
        let samples = files.into_iter().map(|(_, p)| p).collect();
        self.decode_then_run(samples, vec![("project.relinkFiles", json!({ "files": names, "to": to }))]);
    }

    /// Decodes `samples` on another thread, so the window never waits, then runs `commands`
    /// with them at hand (ADR 0011).
    fn decode_then_run(&mut self, samples: Vec<PathBuf>, commands: Vec<(&'static str, serde_json::Value)>) {
        if samples.is_empty() {
            self.run_drop(commands);
            return;
        }
        let (send, decoded) = std::sync::mpsc::channel();
        let count = samples.len();
        let started = std::thread::Builder::new().name("suhina-drop".into()).spawn(move || {
            // A Stems file gives every audio track, each under its stream number.
            let done: Vec<_> = samples
                .into_iter()
                .flat_map(|path| {
                    let decoded: Vec<(Option<u16>, suhina_media::AudioData)> =
                        if suhina_media::stems::is_stems_path(&path) {
                            suhina_media::stems::decode_streams(&path)
                                .map(|all| all.into_iter().zip(0u16..).map(|(d, n)| (Some(n), d)).collect())
                                .unwrap_or_default()
                        } else {
                            suhina_media::decode_file(&path).map(|d| vec![(None, d)]).unwrap_or_default()
                        };
                    decoded.into_iter().map(move |(stream, data)| {
                        let data = std::sync::Arc::new(data);
                        let peaks = crate::waveform::peaks(&data);
                        (path.clone(), stream, data, peaks)
                    })
                })
                .collect();
            let _ = send.send(done);
        });
        match started {
            Ok(_) => {
                self.status = Status::Hint(format!("Reading {count} sample{}", if count == 1 { "" } else { "s" }));
                self.pending_drop = Some(PendingDrop { commands, decoded });
            }
            Err(_) => self.run_drop(commands),
        }
    }

    /// Picks up the samples of a drop once they are decoded and runs its commands.
    fn poll_drop(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_drop else { return };
        match pending.decoded.try_recv() {
            Ok(decoded) => {
                let Some(pending) = self.pending_drop.take() else { return };
                let mut waveforms = (*self.waveforms).clone();
                if let Ok(session) = self.projects.session_mut(None) {
                    for (path, stream, data, peaks) in decoded {
                        let key = std::path::absolute(&path).unwrap_or(path.clone()).display().to_string();
                        let key = match stream {
                            Some(n) => {
                                session.provide_stream(&path, n, data);
                                suhina_engine::model::stream_key(&key, n)
                            }
                            None => {
                                session.provide_audio(&path, data);
                                key
                            }
                        };
                        waveforms.insert(key, std::sync::Arc::new(peaks));
                    }
                }
                self.waveforms = std::sync::Arc::new(waveforms);
                self.run_drop(pending.commands);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(30))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_drop = None;
                self.status = Status::Error("reading the samples stopped".into());
            }
        }
    }

    /// Runs a drop's commands through the open projects, so what was added becomes recent.
    fn run_drop(&mut self, commands: Vec<(&'static str, serde_json::Value)>) {
        for (id, params) in commands {
            match self.projects.execute(id, &params) {
                Ok(result) => {
                    self.revision += 1;
                    self.timeline = None;
                    let skipped = result["skipped"].as_array().map_or(0, Vec::len);
                    let placed = result["clips"].as_array().map_or(0, Vec::len);
                    self.status = match (id, skipped) {
                        ("clip.addFiles", 0) => {
                            Status::Done(format!("Added {placed} clip{}", if placed == 1 { "" } else { "s" }))
                        }
                        ("clip.addFiles", n) => Status::Hint(format!(
                            "Added {placed} clips; {n} skipped: {}",
                            result["skipped"][0]["reason"].as_str().unwrap_or_default()
                        )),
                        _ => Status::Done("Added".into()),
                    };
                }
                Err(e) => {
                    self.status = Status::Error(e.0);
                    break;
                }
            }
        }
    }

    /// How far each column's playing clip is, from the beat it was first seen playing.
    fn progress(&mut self, grid: &Grid) -> Vec<Option<f32>> {
        grid.columns
            .iter()
            .map(|column| {
                let playing = column.slots.iter().position(|s| s.state == SlotState::Playing);
                let Some(slot) = playing else {
                    self.launches.remove(&column.track);
                    return None;
                };
                let started = match self.launches.get(&column.track) {
                    Some((s, beat)) if *s == slot && *beat <= grid.beat => *beat,
                    _ => {
                        self.launches.insert(column.track, (slot, grid.beat));
                        grid.beat
                    }
                };
                let length = column.slots.get(slot).map_or(0.0, |s| s.length);
                (length > 0.0).then(|| (((grid.beat - started) % length) / length) as f32)
            })
            .collect()
    }

    fn control_bar(&mut self, ui: &mut Ui, grid: Option<&Grid>, cpu: Option<u32>, actions: &mut Vec<Action>) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let Some(grid) = grid else {
            widgets::text_in(
                &painter,
                bar.shrink2(Vec2::new(PAD, 0.0)),
                Align2::LEFT_CENTER,
                "Suhina",
                theme::ui_font(theme::TEXT),
                theme::text_secondary(),
            );
            return;
        };
        let row = |x: f32, width: f32| {
            Rect::from_min_size(Pos2::new(x, bar.center().y - theme::ROW / 2.0), Vec2::new(width, theme::ROW))
        };

        // Left: tempo, time signature, launch quantisation.
        let mut x = bar.left() + PAD;
        let tempo_rect = row(x, 56.0);
        let tempo_shown = self.tempo_drag.unwrap_or(grid.tempo_bpm);
        let response = widgets::value_area(ui, tempo_rect, Id::new("tempo"), "Tempo", format!("{tempo_shown:.2}"));
        match widgets::drag_value(ui, &response, &mut self.tempo_drag, grid.tempo_bpm, 0.1, (20.0, 999.0), 120.0) {
            Drag::Done(bpm) => {
                actions.push(Action::new("transport.setTempo", json!({ "bpm": (bpm * 100.0).round() / 100.0 })))
            }
            Drag::Moving(_) | Drag::None => {}
        }
        painter.rect_filled(
            tempo_rect,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        let mono = theme::mono_font(theme::TEXT);
        widgets::text_in(
            &painter,
            tempo_rect.shrink2(Vec2::new(PAD, 0.0)),
            Align2::RIGHT_CENTER,
            &format!("{tempo_shown:.2}"),
            mono.clone(),
            theme::text_primary(),
        );
        x = tempo_rect.right() + PAD;
        let session = self.session();
        let signature = session.map(|s| s.project().transport.time_signature);
        let signature = signature.map_or_else(String::new, |ts| format!("{} / {}", ts.numerator, ts.denominator));
        let ts_rect = row(x, 40.0);
        widgets::text_in(&painter, ts_rect, Align2::CENTER_CENTER, &signature, mono.clone(), theme::text_secondary());
        x = ts_rect.right() + PAD;
        let quant_rect = row(x, 64.0);
        let mut chosen = self.quantization;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            egui::ComboBox::from_id_salt("quantization")
                .width(quant_rect.width())
                .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(theme::text_value()))
                .show_ui(ui, |ui| {
                    for q in QUANTIZATION {
                        ui.selectable_value(&mut chosen, q, egui::RichText::new(q).font(mono.clone()));
                    }
                });
        });
        if chosen != self.quantization {
            actions.push(Action::new("launcher.setQuantization", json!({ "value": chosen })));
        }

        // Centre: play, stop, position.
        let transport_width = 28.0 * 2.0 + 1.0 + PAD + 64.0;
        let mut x = bar.center().x - transport_width / 2.0;
        let play = row(x, 28.0);
        let response = widgets::button_area(ui, play, Id::new("play"), "Play");
        if response.clicked() {
            actions.push(Action::new("transport.play", json!({})));
        }
        let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
        let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(play, 0.0, motion::mix(hover, theme::control_active(), running));
        let glyph = motion::mix(theme::text_primary(), theme::accent(), running);
        let c = play.center();
        painter.add(egui::Shape::convex_polygon(
            vec![c + Vec2::new(-3.5, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(-3.5, 5.0)],
            glyph,
            egui::Stroke::NONE,
        ));
        x = play.right() + 1.0;
        let stop = row(x, 28.0);
        let response = widgets::button_area(ui, stop, Id::new("stop"), "Stop");
        if response.clicked() {
            actions.push(self.stop_action());
        }
        painter.rect_filled(
            stop,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        painter.rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(8.0)), 0.0, theme::text_primary());
        x = stop.right() + PAD;
        let position = row(x, 64.0);
        painter.rect_filled(position, 0.0, theme::window_background());
        widgets::text_in(
            &painter,
            position.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &grid.position(),
            mono,
            theme::text_primary(),
        );

        // Right: the view switch; the current view is underlined. Left of it, the CPU
        // load, from the engine's monitor (per mille, smoothed): how much of the time the
        // audio engine spends rendering.
        let mut right = bar.right() - PAD;
        if let Some(milli) = cpu {
            let rect = row(right - 56.0, 56.0);
            let percent = f64::from(milli.min(1000)) / 10.0;
            painter.rect_filled(rect, 0.0, theme::window_background());
            let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
            widgets::text_in(
                &painter,
                inner,
                Align2::LEFT_CENTER,
                "CPU",
                theme::ui_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
            widgets::text_in(
                &painter,
                inner,
                Align2::RIGHT_CENTER,
                &format!("{percent:.0}%"),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
            let track = Rect::from_min_max(
                Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0),
                Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
            );
            painter.rect_filled(track, 0.0, theme::control_background());
            let fill = track.width() * milli.min(1000) as f32 / 1000.0;
            let full = Rect::from_min_max(track.min, Pos2::new(track.left() + fill, track.bottom()));
            let color = if milli >= 800 { theme::meter_hot() } else { theme::meter_normal() };
            painter.rect_filled(full, 0.0, color);
            let response = ui.interact(rect, Id::new("cpu-load"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("CPU load {percent:.1}%"))
            });
            right = rect.left() - PAD;
        }
        let mut underline_at = None;
        for (view, name) in [(View::Arrangement, "Arrangement"), (View::Session, "Session")] {
            let color = if self.shown == view { theme::text_primary() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, name, theme::ui_font(theme::TEXT), color, 120.0);
            let size = galley.size();
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            let response = widgets::button_area(ui, rect, Id::new(("view", name)), format!("{name} view"));
            if response.clicked() {
                self.shown = view;
            }
            let color = motion::mix(color, theme::text_primary(), widgets::hover(ui, &response));
            let pos = Pos2::new(rect.left(), rect.center().y - size.y / 2.0);
            painter.galley(pos, galley, color);
            if self.shown == view {
                underline_at = Some((pos.x, pos.y + size.y + 1.0, size.x));
            }
            right = rect.left() - 12.0;
        }
        // The underline slides to the view that was picked.
        if let Some((x, y, width)) = underline_at {
            let ctx = ui.ctx();
            let x = motion::value(ctx, Id::new("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::new("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)),
                0.0,
                theme::text_primary(),
            );
        }
        painter.hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline(theme::border()));
    }

    fn status_bar(&self, ui: &mut Ui, grid: Option<&Grid>) {
        let bar = ui.max_rect();
        let painter = ui.painter_at(bar);
        let (text, color) = match &self.status {
            Status::Hint(t) => (t.as_str(), theme::text_secondary()),
            Status::Done(t) => (t.as_str(), theme::text_secondary()),
            Status::Error(t) => (t.as_str(), theme::recording()),
        };
        let right = grid.map(|g| {
            let tracks = g.columns.iter().filter(|c| c.kind != crate::grid::ColumnKind::Return).count();
            format!("{tracks} tracks · {} scenes", g.scenes.len())
        });
        let right_width = if right.is_some() { 140.0 } else { 0.0 };
        let left =
            Rect::from_min_max(bar.min + Vec2::new(PAD, 0.0), Pos2::new(bar.right() - right_width - PAD, bar.bottom()));
        widgets::text_in(&painter, left, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
        if let Some(right) = right {
            let rect =
                Rect::from_min_max(Pos2::new(bar.right() - right_width, bar.top()), bar.max - Vec2::new(PAD, 0.0));
            widgets::text_in(
                &painter,
                rect,
                Align2::RIGHT_CENTER,
                &right,
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
        }
    }
}

/// Darkens the view while a project file is dragged over the window, so it is clear the window
/// takes the drop.
/// Keys are the shortcuts' (Tab switches the view, the arrows move the selection): egui's own
/// keyboard focus, which Tab and the arrows move from widget to widget, is kept to text fields.
/// A button that held it would also make every shortcut wait, as if the user were typing.
fn keys_for_shortcuts(ctx: &egui::Context) {
    let focused = ctx.memory(|m| m.focused());
    let typing = focused.is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
    if !typing {
        ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::None);
            if let Some(id) = focused {
                m.surrender_focus(id);
            }
        });
    }
}

fn drop_overlay(ui: &Ui, area: Rect) {
    let is = |f: &egui::HoveredFile, ext: &str| {
        f.path.as_ref().is_none_or(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
    };
    let (project, themes) = ui.input(|i| {
        let files = &i.raw.hovered_files;
        (
            files.iter().any(|f| is(f, "als") || f.path.as_deref().is_some_and(suhina_session::is_project_file)),
            files.iter().any(|f| is(f, "json") || is(f, "zip")),
        )
    });
    let shown = motion::toward(ui.ctx(), Id::new("drop-overlay"), project || themes, motion::HOVER);
    if shown <= 0.0 {
        return;
    }
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(DROP_DIM * shown));
    let text = theme::text_primary().gamma_multiply(shown);
    // A file whose name is not known yet counts as both; a project is the likelier.
    let what = if themes && !project { "Drop to add the themes" } else { "Drop to open" };
    painter.text(area.center(), Align2::CENTER_CENTER, what, theme::ui_font(theme::TEXT + 2.0), text);
}

/// The window with no project open.
fn empty(ui: &mut Ui) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    painter.text(
        area.center(),
        Align2::CENTER_CENTER,
        "Drop a Live Set (.als) here to open it",
        theme::ui_font(theme::TEXT),
        theme::text_hint(),
    );
}

/// What the status bar says after a command ran, or nothing for silent ones.
fn describe(action: &Action) -> Option<String> {
    let p = &action.params;
    Some(match action.id {
        "clip.launch" => format!("Launched scene {} on track {}", p["scene"], p["track"]),
        "scene.launch" => format!("Launched scene {}", p["scene"]),
        "track.stop" => format!("Stopping track {}", p["track"]),
        "launcher.stopAll" => "Stopping all clips".into(),
        "launcher.setQuantization" => format!("Launch quantisation {}", p["value"].as_str().unwrap_or_default()),
        "transport.setTempo" => format!("Tempo {}", p["bpm"]),
        "edit.undo" => "Undone".into(),
        "edit.redo" => "Redone".into(),
        _ => return None,
    })
}
