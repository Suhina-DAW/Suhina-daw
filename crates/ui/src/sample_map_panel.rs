//! The sample map in the browser (`docs/spec/sample-map.md` › Where the map is shown): which
//! samples it holds (the browser's place), laying them out off the window's thread, and turning
//! what the user does on the map into the browser's selection, previews, drags and commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Rect, Ui};
use serde_json::json;
use suhina_session::Projects;
use suhina_soundmap::Map;
use suhina_soundmap::grid::Grid;

use crate::browser::{BrowserState, Place, ResultsMode};
use crate::drop::{BrowserItem, ItemKind};
use crate::sample_map::{self, Layout, MapEvent, MapPoint, MapView};
use crate::settings::Settings;

/// A scope is laid out again at most this often while its analysis goes on.
const RELAYOUT_SECONDS: f64 = 2.0;

/// What a layout was made from, to know when it is out of date.
#[derive(Debug, Clone, PartialEq)]
struct ScopeKey {
    place: Option<String>,
    max_seconds: u32,
    layout_key: u64,
}

/// The map's state in the window.
pub struct Panel {
    pub view: MapView,
    map: Option<Map>,
    grid: Option<Grid>,
    shown: Option<ScopeKey>,
    /// A layout being computed on a thread of its own.
    job: Option<(ScopeKey, Receiver<(Map, Grid)>)>,
    last_layout: f64,
    points: Vec<MapPoint>,
    /// What the points' flags were made from: search, tags, selection, pins, project.
    flags_key: Option<String>,
    /// An item double-clicked, for the window to add to the selected track.
    pub add: Vec<BrowserItem>,
    /// Something to say in the status bar (a preview that cannot play).
    pub status: Option<String>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            view: MapView::default(),
            map: None,
            grid: None,
            shown: None,
            job: None,
            last_layout: f64::NEG_INFINITY,
            points: Vec::new(),
            flags_key: None,
            add: Vec::new(),
            status: None,
        }
    }
}

fn item_of(path: &Path) -> BrowserItem {
    BrowserItem {
        key: path.display().to_string(),
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        kind: ItemKind::Sample,
        plugin: false,
    }
}

/// The files the active project's audio clips use, as absolute paths.
fn files_in_use(projects: &Projects) -> HashSet<PathBuf> {
    let Some(session) = projects.active().and_then(|id| projects.get(id)) else { return HashSet::new() };
    let dir = session.source().and_then(Path::parent).map(Path::to_path_buf);
    let mut out = HashSet::new();
    for content in &session.project().contents {
        for lane in &content.lanes {
            let suhina_engine::model::LaneData::Audio(region) = &lane.data else { continue };
            if let Some(absolute) = &region.absolute_file {
                out.insert(PathBuf::from(absolute));
            }
            if let Some(dir) = &dir {
                out.insert(dir.join(&region.file));
            }
            out.insert(PathBuf::from(&region.file));
        }
    }
    out
}

impl Panel {
    /// The browser's place as the `soundmap.*` commands take it.
    fn place(state: &BrowserState) -> Option<String> {
        match &state.place {
            None => None,
            Some(Place::Pinned) => Some("pinned".into()),
            Some(place @ Place::Recent(_)) => Some(place.param()),
            Some(Place::Folder(path)) => Some(path.display().to_string()),
        }
    }

    /// Keeps the layout in line with the place, the analysis and the settings: takes in a
    /// finished layout and starts a new one when it is out of date.
    fn refresh(&mut self, ui: &Ui, state: &BrowserState, projects: &mut Projects, settings: &Settings) {
        let analysed = projects.poll_sound_map();
        if let Some((key, receiver)) = &self.job {
            match receiver.try_recv() {
                Ok((map, grid)) => {
                    projects.sound_map_mut().keep(&map);
                    self.shown = Some(key.clone());
                    self.map = Some(map);
                    self.grid = Some(grid);
                    self.flags_key = None;
                    self.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.job = None,
            }
        }
        let now = ui.input(|i| i.time);
        let place = Self::place(state);
        let scope_changed =
            self.shown.as_ref().is_none_or(|s| s.place != place || s.max_seconds != settings.map_max_seconds);
        if !scope_changed && !(analysed && now - self.last_layout >= RELAYOUT_SECONDS) {
            if analysed {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(RELAYOUT_SECONDS));
            }
            return;
        }
        let Ok(files) = suhina_session::scope_files(projects, place.as_deref()) else { return };
        let prepared = projects.sound_map_mut().prepare(&files, settings.map_max_seconds as f32);
        let key = ScopeKey { place, max_seconds: settings.map_max_seconds, layout_key: prepared.key() };
        self.last_layout = now;
        if self.shown.as_ref() == Some(&key) {
            return;
        }
        if prepared.is_laid_out() {
            let map = prepared.compute();
            self.grid = Some(map.grid(1.0));
            self.map = Some(map);
            self.shown = Some(key);
            self.flags_key = None;
            return;
        }
        let (sender, receiver) = channel();
        let ctx = ui.ctx().clone();
        let spawned = std::thread::Builder::new().name("soundmap-layout".into()).spawn(move || {
            let map = prepared.compute();
            let grid = map.grid(1.0);
            let _ = sender.send((map, grid));
            ctx.request_repaint();
        });
        if spawned.is_ok() {
            self.job = Some((key, receiver));
        }
    }

    /// Rebuilds the points when the map changed, and their flags when the search, the tags,
    /// the selection, the pins or the project changed.
    fn update_points(&mut self, state: &BrowserState, projects: &mut Projects, revision: u64) {
        let Some(map) = &self.map else {
            self.points.clear();
            return;
        };
        let selected: HashSet<&str> = state.selected().iter().map(|i| i.key.as_str()).collect();
        let pinned: HashSet<String> = projects.file_library_mut().pinned().into_iter().collect();
        let key = format!(
            "{}\u{1}{:?}\u{1}{:?}\u{1}{}\u{1}{}\u{1}{}",
            state.search,
            state.tag_filter,
            selected.iter().collect::<std::collections::BTreeSet<_>>(),
            pinned.len(),
            revision,
            projects.file_library_mut().revision(),
        );
        if self.flags_key.as_deref() == Some(key.as_str()) && self.points.len() == map.points.len() {
            return;
        }
        let words = suhina_library::words(&state.search);
        let in_use = files_in_use(projects);
        let library = projects.file_library_mut();
        self.points = map
            .points
            .iter()
            .map(|p| {
                let path_key = p.path.display().to_string();
                let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let tags = library.item_tags(&path_key);
                let text_matches = words.is_empty() || suhina_library::rank(&words, &name, &path_key).is_some();
                let tag_matches = state.tag_filter.is_empty() || tags.iter().any(|t| state.tag_filter.contains(t));
                let folder = p.path.parent().map(|f| f.display().to_string()).unwrap_or_default();
                MapPoint {
                    detail: folder,
                    name,
                    position: p.position,
                    kind: p.kind,
                    guessed: p.guessed,
                    in_use: in_use.contains(&p.path),
                    selected: selected.contains(path_key.as_str()),
                    matches: text_matches && tag_matches,
                    preset: false,
                    pinned: pinned.contains(&path_key),
                    tags,
                }
            })
            .collect();
        self.flags_key = Some(key);
    }

    fn status(&self) -> String {
        let Some(map) = &self.map else {
            return if self.job.is_some() { "Arranging…".into() } else { "No samples analysed yet".into() };
        };
        let mut parts = vec![format!("{} samples", map.points.len())];
        if self.job.is_some() {
            parts.push("arranging…".into());
        }
        if map.pending > 0 {
            parts.push(format!("analysing, {} to go", map.pending));
        }
        if map.too_long > 0 {
            parts.push(format!("{} too long", map.too_long));
        }
        if map.failed > 0 {
            parts.push(format!("{} could not be read", map.failed));
        }
        if map.over_limit > 0 {
            parts.push(format!("{} left out", map.over_limit));
        }
        parts.join(", ")
    }

    /// Moves the map into a window of its own (`true`) or back into the browser. While it has a
    /// window the browser shows its list, as it does for the large map.
    pub fn set_window(&mut self, window: bool, state: &mut BrowserState) {
        self.view.window = window;
        self.view.large = false;
        state.mode = if window { ResultsMode::List } else { ResultsMode::Other };
    }

    /// Draws the map in `rect` (the browser's results area, or the main area when large).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        state: &mut BrowserState,
        projects: &mut Projects,
        settings: &mut Settings,
        revision: u64,
    ) -> bool {
        self.refresh(ui, state, projects, settings);
        self.update_points(state, projects, revision);
        self.view.layout = if settings.map_grid { Layout::Grid } else { Layout::Cloud };
        self.view.preview_on_hover = settings.map_hover_preview;
        let tags: Vec<String> = projects.file_library_mut().tags().into_iter().map(|t| t.name).collect();
        let status = self.status();
        let mut events = Vec::new();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            sample_map::show(ui, &mut self.view, &self.points, self.grid.as_ref(), &status, &tags, &mut events);
        });
        let mut settings_changed = false;
        for event in events {
            settings_changed |= self.handle(event, state, projects, settings);
        }
        settings_changed
    }

    fn path(&self, index: usize) -> Option<PathBuf> {
        self.map.as_ref()?.points.get(index).map(|p| p.path.clone())
    }

    /// Acts on one event; returns whether a setting changed.
    fn handle(
        &mut self,
        event: MapEvent,
        state: &mut BrowserState,
        projects: &mut Projects,
        settings: &mut Settings,
    ) -> bool {
        match event {
            MapEvent::Preview(i) => {
                if let Some(path) = self.path(i)
                    && let Err(why) = preview(state, projects, path)
                {
                    self.status = Some(why);
                }
            }
            MapEvent::Select { items, add } => {
                let picked: Vec<BrowserItem> =
                    items.iter().filter_map(|i| self.path(*i)).map(|p| item_of(&p)).collect();
                let selection = if add {
                    let mut current = state.selected().to_vec();
                    for item in picked {
                        match current.iter().position(|c| c.key == item.key) {
                            Some(at) => {
                                current.remove(at);
                            }
                            None => current.push(item),
                        }
                    }
                    current
                } else {
                    picked
                };
                // The map previews by itself; the selection must not preview again.
                state.select(selection);
                state.selection_changed = false;
            }
            MapEvent::StartDrag(items) => {
                let items: Vec<BrowserItem> = items.iter().filter_map(|i| self.path(*i)).map(|p| item_of(&p)).collect();
                state.start_drag(items);
            }
            MapEvent::Add(i) => {
                if let Some(path) = self.path(i) {
                    self.add.push(item_of(&path));
                }
            }
            MapEvent::Similar(i) => {
                if let (Some(map), Some(path)) = (&self.map, self.path(i)) {
                    let similar: Vec<usize> = map
                        .similar(&path, 16)
                        .into_iter()
                        .filter_map(|(p, _)| map.points.iter().position(|q| q.path == p))
                        .collect();
                    self.view.show_similar(i, similar);
                }
            }
            MapEvent::Pin(i) => {
                if let Some(path) = self.path(i) {
                    let key = path.display().to_string();
                    let pinned = projects.file_library_mut().pinned().contains(&key);
                    let id = if pinned { "library.unpin" } else { "library.pin" };
                    let _ = projects.execute(id, &json!({ "item": key }));
                    self.flags_key = None;
                }
            }
            MapEvent::Tag { item, tag, on } => {
                if let Some(path) = self.path(item) {
                    let _ = projects
                        .execute("library.tag", &json!({ "item": path.display().to_string(), "tag": tag, "on": on }));
                    self.flags_key = None;
                }
            }
            MapEvent::ShowInFolder(i) => {
                if let Some(path) = self.path(i) {
                    if let Some(folder) = path.parent() {
                        state.place = Some(Place::Folder(folder.to_path_buf()));
                    }
                    state.mode = ResultsMode::List;
                    self.view.large = false;
                    state.select(vec![item_of(&path)]);
                    state.selection_changed = false;
                }
            }
            MapEvent::Layout(layout) => {
                settings.map_grid = layout == Layout::Grid;
                return true;
            }
            MapEvent::ToggleLarge => {
                self.view.large = !self.view.large;
                // The large map takes the main area; the browser shows its list meanwhile.
                state.mode = if self.view.large { ResultsMode::List } else { ResultsMode::Other };
            }
            MapEvent::ToggleWindow => {
                self.set_window(!self.view.window, state);
                settings.map_window = self.view.window;
                return true;
            }
        }
        false
    }
}

/// Previews a sample as the browser does: decoded on the browser's reader thread, then played.
fn preview(state: &mut BrowserState, projects: &mut Projects, path: PathBuf) -> Result<(), String> {
    crate::browser_view::can_preview(projects)?;
    crate::browser_view::preview_sample(state, projects, path, 0.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_session::{Session, blank_project};

    fn temp(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("suhina-map-panel-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wav(path: &Path, hz: f32, seconds: f32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(seconds * 22_050.0) as usize {
            let t = i as f32 / 22_050.0;
            let s = (std::f32::consts::TAU * hz * t).sin() * (-t * 6.0).exp();
            writer.write_sample((s * 20_000.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    /// Projects whose library holds a folder of analysed samples: kicks (low) and hats (high).
    fn analysed() -> (Projects, PathBuf, PathBuf) {
        let settings = temp("settings");
        let samples = temp("samples");
        for i in 0..8 {
            wav(&samples.join(format!("kick {i}.wav")), 50.0 + i as f32 * 3.0, 0.5);
            wav(&samples.join(format!("hat {i}.wav")), 6000.0 + i as f32 * 100.0, 0.1);
        }
        wav(&samples.join("long kick.wav"), 55.0, 3.0);
        let mut projects = Projects::new();
        projects.set_file_library(suhina_library::Library::new(Some(settings.clone())));
        projects.add(Session::new(blank_project()), None).unwrap();
        projects.execute("library.addFolder", &json!({ "folder": samples.display().to_string() })).unwrap();
        projects.start_sound_map();
        projects.wait_sound_map();
        (projects, settings, samples)
    }

    #[test]
    fn the_map_shows_the_place_and_acts_on_the_browser() {
        let (mut projects, settings_dir, samples) = analysed();
        let mut panel = Panel::default();
        let mut state = BrowserState::default();
        state.mode = ResultsMode::Other;
        let mut settings = Settings { map_max_seconds: 1, ..Settings::default() };
        let ctx = egui::Context::default();
        let draw = |panel: &mut Panel, state: &mut BrowserState, projects: &mut Projects, settings: &mut Settings| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let rect = ui.max_rect();
                panel.show(ui, rect, state, projects, settings, 0);
            });
            output.textures_delta.clear();
        };
        draw(&mut panel, &mut state, &mut projects, &mut settings);
        // The layout runs on a thread of its own; it arrives on a later frame.
        for _ in 0..200 {
            if panel.map.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            draw(&mut panel, &mut state, &mut projects, &mut settings);
        }
        let map = panel.map.as_ref().expect("laid out");
        assert_eq!(map.points.len(), 16);
        assert_eq!(map.too_long, 1);
        assert!(panel.status().starts_with("16 samples"), "{}", panel.status());
        assert_eq!(panel.points.len(), 16);
        let kick = panel.points.iter().position(|p| p.name == "kick 0.wav").unwrap();
        assert_eq!(panel.points[kick].kind, suhina_soundmap::kinds::Kind::Kick);

        // Selecting on the map selects in the browser; the search dims what does not match.
        panel.handle(MapEvent::Select { items: vec![kick], add: false }, &mut state, &mut projects, &mut settings);
        assert_eq!(state.selected().len(), 1);
        assert!(state.selected()[0].key.ends_with("kick 0.wav"));
        state.search = "hat".into();
        draw(&mut panel, &mut state, &mut projects, &mut settings);
        assert!(panel.points.iter().all(|p| p.matches == p.name.starts_with("hat")));
        assert!(panel.points[kick].selected);

        // Pins, tags, similar items, drags and double-clicks.
        panel.handle(MapEvent::Pin(kick), &mut state, &mut projects, &mut settings);
        assert_eq!(projects.file_library_mut().pinned().len(), 1);
        panel.handle(MapEvent::Similar(kick), &mut state, &mut projects, &mut settings);
        let (_, similar) = panel.view.similar().unwrap();
        assert!(similar.iter().take(5).all(|i| panel.points[*i].name.contains("kick")));
        panel.handle(MapEvent::StartDrag(vec![kick]), &mut state, &mut projects, &mut settings);
        assert_eq!(state.drag.as_ref().map(Vec::len), Some(1));
        panel.handle(MapEvent::Add(kick), &mut state, &mut projects, &mut settings);
        assert_eq!(panel.add.len(), 1);

        // The layout switch is a setting; the large view hands the results back to the list.
        assert!(panel.handle(MapEvent::Layout(Layout::Grid), &mut state, &mut projects, &mut settings));
        assert!(settings.map_grid);
        panel.handle(MapEvent::ToggleLarge, &mut state, &mut projects, &mut settings);
        assert_eq!((panel.view.large, state.mode), (true, ResultsMode::List));
        panel.handle(MapEvent::ShowInFolder(kick), &mut state, &mut projects, &mut settings);
        assert_eq!(state.place, Some(Place::Folder(samples.clone())));
        assert!(!panel.view.large);

        // A longer limit takes in the long sample.
        settings.map_max_seconds = 10;
        state.place = None;
        for _ in 0..200 {
            draw(&mut panel, &mut state, &mut projects, &mut settings);
            if panel.map.as_ref().is_some_and(|m| m.points.len() == 17) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(panel.map.as_ref().unwrap().points.len(), 17);
        drop(projects);
        let _ = std::fs::remove_dir_all(settings_dir);
        let _ = std::fs::remove_dir_all(samples);
    }
}
