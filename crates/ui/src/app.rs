//! The main window: tabs for the open projects (ADR 0007), the control bar, the Session or
//! Arrangement view and the status bar around the active project.
//! Every change goes through `Session::execute` (AGENTS.md rule 6); the window only reads the
//! project and the engine's monitor to draw them.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Align2, Id, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use loopwerk_engine::model::TrackId;
use loopwerk_session::{CommandKind, ProjectId, Projects, Session};
use serde_json::json;

use crate::arrangement_view::{self, ArrangementViewState};
use crate::grid::{Grid, SlotState};
use crate::motion;
use crate::session_view::{self, SessionViewState};
use crate::settings::{OnOpen, Settings, Side};
use crate::settings_view::{self, About, PluginRequest, PluginsView};
use crate::theme::{self, PAD};
use crate::timeline::Timeline;
use crate::widgets::{self, Action, Drag};

/// Launch quantisation choices, as `launcher.setQuantization` takes them.
const QUANTIZATION: [&str; 14] = [
    "none", "8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32",
];
/// The engine's quantisation before anything is set.
const DEFAULT_QUANTIZATION: &str = "1 bar";

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
const NEXT_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Tab);
const PREVIOUS_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Tab);
/// Height of the tab row, shown while two or more projects are open.
const TABS: f32 = 22.0;
/// How dark the window gets while a project file is dragged over it.
const DROP_DIM: f32 = 0.45;

/// How a project the user asked to open is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAs {
    NewTab,
    /// In place of the active project, which is closed.
    Replace,
}

/// A finished plug-in scan: the library with its new catalog, and how many files were read and
/// failed, or why the scan could not run.
struct ScanDone {
    library: loopwerk_plugins::library::Library,
    report: Result<(usize, usize), String>,
}

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
    settings: Settings,
    /// The settings changed and should be saved.
    settings_changed: bool,
    /// The Settings page is showing instead of the view.
    settings_open: bool,
    about: About,
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
            settings: Settings::default(),
            settings_changed: false,
            settings_open: false,
            about: About::default(),
        }
    }

    /// A window whose `plugins.*` commands use `library` instead of the one in Loopwerk's
    /// settings folder (tests, portable setups).
    pub fn with_library(library: loopwerk_plugins::library::Library) -> Self {
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
    pub fn attach_player(&mut self, id: ProjectId, player: loopwerk_session::Player) {
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
    }

    /// The waveforms of the open project's audio files, once they are decoded.
    pub fn set_waveforms(&mut self, waveforms: crate::waveform::Waveforms) {
        self.waveforms = std::sync::Arc::new(waveforms);
        self.timeline = None;
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
                let changes_project = loopwerk_session::commands()
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

    /// Draws the window into `ui` and runs what the user asked for.
    pub fn ui(&mut self, ui: &mut Ui) {
        let mut actions = Vec::new();
        self.follow_active();
        self.poll_scan(ui.ctx());
        self.dropped_files(ui);
        self.shortcuts(ui, &mut actions);

        let grid = self.session().map(|s| Grid::build(s.project(), s.player().map(|p| p.monitor())));
        let progress = grid.as_ref().map(|g| self.progress(g)).unwrap_or_default();

        egui::Panel::top("menu-bar")
            .exact_size(theme::MENU_BAR)
            .frame(egui::Frame::NONE.fill(theme::SURFACE_1).inner_margin(egui::Margin::symmetric(4, 0)))
            .show(ui, |ui| self.menu_bar(ui, &mut actions));
        if self.projects.len() >= 2 {
            egui::Panel::top("tabs")
                .exact_size(TABS)
                .frame(egui::Frame::NONE.fill(theme::SURFACE_0))
                .show(ui, |ui| self.tab_row(ui, &mut actions));
        }
        egui::Panel::top("control-bar")
            .exact_size(theme::CONTROL_BAR)
            .frame(egui::Frame::NONE.fill(theme::SURFACE_1))
            .show(ui, |ui| self.control_bar(ui, grid.as_ref(), &mut actions));
        egui::Panel::bottom("status-bar")
            .exact_size(theme::STATUS_BAR)
            .frame(egui::Frame::NONE.fill(theme::SURFACE_1))
            .show(ui, |ui| self.status_bar(ui, grid.as_ref()));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::LINE)).show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            // A 1 px gap shows the line colour between the bars and the view.
            let inner = Rect::from_min_max(area.min + Vec2::new(0.0, 1.0), area.max - Vec2::new(0.0, 1.0));
            if self.settings_open {
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| self.settings_page(ui));
                return;
            }
            let prompt_height = if self.open_prompt.is_some() { theme::ROW + 12.0 } else { 0.0 };
            let (prompt, inner) = (
                Rect::from_min_size(inner.min, Vec2::new(inner.width(), prompt_height)),
                Rect::from_min_max(inner.min + Vec2::new(0.0, prompt_height), inner.max),
            );
            if self.open_prompt.is_some() {
                ui.scope_builder(egui::UiBuilder::new().max_rect(prompt), |ui| self.open_prompt_bar(ui));
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| match (&grid, self.shown) {
                (Some(grid), View::Session) => session_view::show(ui, grid, &progress, &mut self.view, &mut actions),
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
            drop_overlay(ui, inner);
        });

        for action in &actions {
            self.run(action);
        }
        if grid.is_some_and(|g| {
            g.running || g.columns.iter().any(|c| c.stop_queued || c.slots.iter().any(|s| s.state == SlotState::Queued))
        }) {
            ui.ctx().request_repaint();
        }
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
            let galley = widgets::fitted(&painter, &name, theme::ui_font(theme::TEXT), theme::TEXT_PRIMARY, 180.0);
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
            let idle = motion::mix(theme::SURFACE_0, theme::HOVER_ON_SURFACE_1, widgets::hover(ui, &response));
            painter.rect_filled(tab, 0.0, motion::mix(idle, theme::SURFACE_1, lit));
            let text = motion::mix(theme::TEXT_SECONDARY, theme::TEXT_PRIMARY, lit);
            painter.galley(Pos2::new(tab.left() + PAD, tab.center().y - galley.size().y / 2.0), galley, text);
            let cross = motion::mix(theme::TEXT_TERTIARY, theme::TEXT_PRIMARY, widgets::hover(ui, &close_response));
            let c = close.center();
            for (a, b) in [(Vec2::new(-3.0, -3.0), Vec2::new(3.0, 3.0)), (Vec2::new(-3.0, 3.0), Vec2::new(3.0, -3.0))] {
                painter.line_segment([c + a, c + b], theme::hairline(cross));
            }
            painter.vline(tab.right() + 0.5, bar.y_range(), theme::hairline(theme::LINE));
        }
    }

    /// The question asked when a project is dropped while another is open.
    fn open_prompt_bar(&mut self, ui: &mut Ui) {
        let Some(path) = self.open_prompt.clone() else { return };
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, theme::SURFACE_1);
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        let current = self.projects.active().and_then(|id| self.projects.name(id)).unwrap_or("the project").to_string();
        let mut choice = None;
        let mut cancel = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(PAD, 6.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(format!("Open {name}:")).color(theme::TEXT_PRIMARY));
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
                let started = std::thread::Builder::new().name("loopwerk-plugin-scan".into()).spawn(move || {
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

    /// Whether a plug-in scan is running.
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// The Settings page, applying each change as it is made.
    fn settings_page(&mut self, ui: &mut Ui) {
        if !self.plugins_read {
            self.read_plugins();
        }
        let outcome = settings_view::show(ui, &mut self.settings, &self.about, &mut self.plugins);
        if let Some(request) = outcome.plugins.clone() {
            self.plugin_request(request);
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
        ui.style_mut().visuals.widgets.inactive.fg_stroke = theme::hairline(theme::TEXT_SECONDARY);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(can_undo, egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                    actions.push(Action::new("edit.undo", json!({})));
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text("Ctrl+Shift+Z")).clicked() {
                    actions.push(Action::new("edit.redo", json!({})));
                }
            });
            ui.menu_button("View", |ui| {
                if ui.add_enabled(has_project, egui::Button::new("Session").shortcut_text("Tab")).clicked() {
                    self.shown = View::Session;
                    self.settings_open = false;
                }
                if ui.add_enabled(has_project, egui::Button::new("Arrangement").shortcut_text("Tab")).clicked() {
                    self.shown = View::Arrangement;
                    self.settings_open = false;
                }
            });
            ui.menu_button("Projects", |ui| {
                if ui.add_enabled(several, egui::Button::new("Next project").shortcut_text("Ctrl+Tab")).clicked() {
                    actions.push(Action::new("projects.next", json!({})));
                }
                if ui
                    .add_enabled(several, egui::Button::new("Previous project").shortcut_text("Ctrl+Shift+Tab"))
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
                if ui.add(egui::Button::new("Settings").shortcut_text("Ctrl+,")).clicked() {
                    self.settings_open = true;
                }
            });
        });
    }

    fn dropped_files(&mut self, ui: &Ui) {
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("als")) {
                self.request_open(path);
            } else {
                self.status = Status::Error(format!("{} is not a Live Set (.als)", path.display()));
            }
        }
    }

    fn shortcuts(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        if self.projects.is_empty() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (next, previous) = ui.input_mut(|i| (i.consume_shortcut(&NEXT_TAB), i.consume_shortcut(&PREVIOUS_TAB)));
        if previous && self.projects.len() >= 2 {
            actions.push(Action::new("projects.previous", json!({})));
        } else if next && self.projects.len() >= 2 {
            actions.push(Action::new("projects.next", json!({})));
        }
        if ui.input_mut(|i| i.consume_shortcut(&SETTINGS)) {
            self.settings_open = !self.settings_open;
        }
        ui.input_mut(|i| {
            if i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_Y) {
                actions.push(Action::new("edit.redo", json!({})));
            } else if i.consume_shortcut(&UNDO) {
                actions.push(Action::new("edit.undo", json!({})));
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) {
                self.shown = match self.shown {
                    View::Session => View::Arrangement,
                    View::Arrangement => View::Session,
                };
            }
            if i.consume_key(Modifiers::NONE, Key::Space) {
                let running = self.session().and_then(|s| s.player()).is_some_and(|p| p.monitor().running());
                actions.push(Action::new(if running { "transport.stop" } else { "transport.play" }, json!({})));
            }
        });
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

    fn control_bar(&mut self, ui: &mut Ui, grid: Option<&Grid>, actions: &mut Vec<Action>) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let Some(grid) = grid else {
            widgets::text_in(
                &painter,
                bar.shrink2(Vec2::new(PAD, 0.0)),
                Align2::LEFT_CENTER,
                "Loopwerk",
                theme::ui_font(theme::TEXT),
                theme::TEXT_SECONDARY,
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
            motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response)),
        );
        let mono = theme::mono_font(theme::TEXT);
        widgets::text_in(
            &painter,
            tempo_rect.shrink2(Vec2::new(PAD, 0.0)),
            Align2::RIGHT_CENTER,
            &format!("{tempo_shown:.2}"),
            mono.clone(),
            theme::TEXT_PRIMARY,
        );
        x = tempo_rect.right() + PAD;
        let session = self.session();
        let signature = session.map(|s| s.project().transport.time_signature);
        let signature = signature.map_or_else(String::new, |ts| format!("{} / {}", ts.numerator, ts.denominator));
        let ts_rect = row(x, 40.0);
        widgets::text_in(&painter, ts_rect, Align2::CENTER_CENTER, &signature, mono.clone(), theme::TEXT_SECONDARY);
        x = ts_rect.right() + PAD;
        let quant_rect = row(x, 64.0);
        let mut chosen = self.quantization;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            egui::ComboBox::from_id_salt("quantization")
                .width(quant_rect.width())
                .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(theme::TEXT_VALUE))
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
        let hover = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response));
        painter.rect_filled(play, 0.0, motion::mix(hover, theme::SURFACE_3, running));
        let glyph = motion::mix(theme::TEXT_PRIMARY, theme::ACCENT, running);
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
            actions.push(Action::new("transport.stop", json!({})));
        }
        painter.rect_filled(
            stop,
            0.0,
            motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response)),
        );
        painter.rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(8.0)), 0.0, theme::TEXT_PRIMARY);
        x = stop.right() + PAD;
        let position = row(x, 64.0);
        painter.rect_filled(position, 0.0, theme::SURFACE_0);
        widgets::text_in(
            &painter,
            position.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &grid.position(),
            mono,
            theme::TEXT_PRIMARY,
        );

        // Right: the view switch; the current view is underlined.
        let mut right = bar.right() - PAD;
        let mut underline_at = None;
        for (view, name) in [(View::Arrangement, "Arrangement"), (View::Session, "Session")] {
            let color = if self.shown == view { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY };
            let galley = widgets::fitted(&painter, name, theme::ui_font(theme::TEXT), color, 120.0);
            let size = galley.size();
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            let response = widgets::button_area(ui, rect, Id::new(("view", name)), format!("{name} view"));
            if response.clicked() {
                self.shown = view;
            }
            let color = motion::mix(color, theme::TEXT_PRIMARY, widgets::hover(ui, &response));
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
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)), 0.0, theme::TEXT_PRIMARY);
        }
        painter.hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline(theme::LINE));
    }

    fn status_bar(&self, ui: &mut Ui, grid: Option<&Grid>) {
        let bar = ui.max_rect();
        let painter = ui.painter_at(bar);
        let (text, color) = match &self.status {
            Status::Hint(t) => (t.as_str(), theme::TEXT_SECONDARY),
            Status::Done(t) => (t.as_str(), theme::TEXT_SECONDARY),
            Status::Error(t) => (t.as_str(), theme::RECORDING),
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
                theme::TEXT_TERTIARY,
            );
        }
    }
}

/// Darkens the view while a project file is dragged over the window, so it is clear the window
/// takes the drop.
fn drop_overlay(ui: &Ui, area: Rect) {
    let hovering = ui.input(|i| {
        i.raw
            .hovered_files
            .iter()
            .any(|f| f.path.as_ref().is_none_or(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("als"))))
    });
    let shown = motion::toward(ui.ctx(), Id::new("drop-overlay"), hovering, motion::HOVER);
    if shown <= 0.0 {
        return;
    }
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::SURFACE_0.gamma_multiply(DROP_DIM * shown));
    let text = theme::TEXT_PRIMARY.gamma_multiply(shown);
    painter.text(area.center(), Align2::CENTER_CENTER, "Drop to open", theme::ui_font(theme::TEXT + 2.0), text);
}

/// The window with no project open.
fn empty(ui: &mut Ui) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::SURFACE_0);
    painter.text(
        area.center(),
        Align2::CENTER_CENTER,
        "Drop a Live Set (.als) here to open it",
        theme::ui_font(theme::TEXT),
        theme::TEXT_TERTIARY,
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
