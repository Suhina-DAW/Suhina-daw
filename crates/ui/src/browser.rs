//! The browser's state and what it lists (`docs/spec/browser.md`): the search, the category,
//! the place chosen in the sidebar, the open folders of the tree, the selection, and what is
//! known about files (length, tempo, key, waveform), read on a thread of its own so the window
//! never waits for the disk. `browser_view` draws it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use suhina_engine::AudioData;
use suhina_library::{FileKind, Group, Item, Listing};
use suhina_media::info::{Tempo, TempoSource};
use suhina_session::{Category, Found, Projects};

use crate::drop::{BrowserItem, ItemKind};
use crate::waveform::{Peaks, peaks};

/// What the sidebar can show instead of the whole tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Place {
    Pinned,
    Recent(Group),
    Folder(PathBuf),
}

impl Place {
    /// As `library.search` takes it.
    pub fn param(&self) -> String {
        match self {
            Self::Pinned => "pinned".into(),
            Self::Recent(group) => format!("recent:{}", group.name()),
            Self::Folder(path) => path.display().to_string(),
        }
    }
}

/// How the results area shows what was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsMode {
    /// The list or tree of rows.
    #[default]
    List,
    /// Another view, drawn by `browser_view::Context::other_results`.
    Other,
}

/// One row of the results.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub item: BrowserItem,
    pub depth: usize,
    /// It opens (a folder, a heading, a preset bank).
    pub opens: bool,
    pub open: bool,
    /// What it is shown under in a flat list (the folder a search found it in).
    pub context: Option<String>,
}

/// What is known about a file, as it is read in the background.
#[derive(Debug, Clone, Default)]
pub struct FileInfo {
    pub seconds: Option<f64>,
    pub tempo: Option<Tempo>,
    pub key: Option<String>,
    /// The decoded sample and its waveform, once read whole (only for the selected file).
    pub data: Option<Arc<AudioData>>,
    pub peaks: Option<Arc<Peaks>>,
    /// A MIDI file's notes as (start, length, key) in beats, and its length in beats.
    pub notes: Option<Arc<Vec<(f64, f64, u8)>>>,
    pub beats: f64,
    pub error: Option<String>,
}

enum Request {
    /// The header: length, rate, tempo and key from metadata or the name.
    Header(PathBuf),
    /// Everything: decoded, with its waveform and a detected tempo.
    Whole(PathBuf),
}

struct Answer {
    path: PathBuf,
    whole: bool,
    info: FileInfo,
}

/// Files whose headers are kept; the oldest are forgotten first.
const MAX_INFO: usize = 4096;
/// Samples longer than this get no tempo detection in the browser.
const DETECT_SECONDS: f64 = 60.0;

/// Reads files on a thread of its own.
struct Reader {
    to: Option<Sender<Request>>,
    from: Option<Receiver<Answer>>,
}

impl Reader {
    fn start() -> Self {
        let (to, requests) = std::sync::mpsc::channel::<Request>();
        let (answers, from) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new().name("suhina-browser-files".into()).spawn(move || {
            while let Ok(request) = requests.recv() {
                let (path, whole) = match request {
                    Request::Header(path) => (path, false),
                    Request::Whole(path) => (path, true),
                };
                let info = read_info(&path, whole);
                if answers.send(Answer { path, whole, info }).is_err() {
                    return;
                }
            }
        });
        match started {
            Ok(_) => Self { to: Some(to), from: Some(from) },
            Err(_) => Self { to: None, from: None },
        }
    }
}

fn read_info(path: &Path, whole: bool) -> FileInfo {
    let mut info = FileInfo::default();
    match suhina_media::info::kind_of(path) {
        Some(FileKind::Sample | FileKind::Stems) => {
            match suhina_media::info::sample_info(path) {
                Ok(header) => {
                    info.seconds = header.duration_seconds;
                    info.tempo = header.tempo;
                    info.key = header.key;
                }
                Err(e) => info.error = Some(e),
            }
            if whole {
                match suhina_media::decode_file(path) {
                    Ok(data) => {
                        let seconds = data.frames() as f64 / f64::from(data.sample_rate.max(1));
                        info.seconds = Some(seconds);
                        if info.tempo.is_none() && seconds <= DETECT_SECONDS {
                            info.tempo = suhina_media::tempo::detect(&data)
                                .map(|bpm| Tempo { bpm, source: TempoSource::Detected });
                        }
                        info.peaks = Some(Arc::new(peaks(&data)));
                        info.data = Some(Arc::new(data));
                        info.error = None;
                    }
                    Err(e) => info.error = Some(e),
                }
            }
        }
        Some(FileKind::Midi) => match suhina_media::midi::read_file(path) {
            Ok(midi) => {
                let notes: Vec<(f64, f64, u8)> =
                    midi.tracks.iter().flat_map(|t| t.notes.iter().map(|n| (n.time, n.duration, n.key))).collect();
                info.beats = midi.tracks.iter().map(|t| t.end()).fold(0.0, f64::max);
                let bpm = midi.tempo_bpm.unwrap_or(120.0);
                info.seconds = Some(info.beats * 60.0 / bpm);
                info.tempo = midi.tempo_bpm.map(|bpm| Tempo { bpm, source: TempoSource::Metadata });
                info.notes = Some(Arc::new(notes));
            }
            Err(e) => info.error = Some(e),
        },
        _ => {}
    }
    info
}

/// The browser panel's state. One for the window: it is the same whichever project shows.
pub struct BrowserState {
    pub mode: ResultsMode,
    pub search: String,
    pub category: Category,
    pub place: Option<Place>,
    /// Open nodes: folders by path, headings and vendors by name.
    pub open: HashSet<String>,
    pub selected: Vec<BrowserItem>,
    /// The row last clicked, for Shift+click.
    pub anchor: Option<usize>,
    /// Only items carrying any of these tags.
    pub tag_filter: Vec<String>,
    /// Put the cursor in the search field on the next frame.
    pub focus_search: bool,
    /// The search field had the keyboard last frame.
    pub search_focused: bool,
    /// Escape in the search field asked for the preview to stop.
    pub stop_preview: bool,
    /// The sidebar's field for a folder to add, while it shows.
    pub new_folder: Option<String>,
    /// Items being dragged into the project.
    pub drag: Option<Vec<BrowserItem>>,
    /// The file previewing, by key.
    pub previewing: Option<String>,
    /// A sample waiting to be read whole before it previews.
    pending_preview: Option<(PathBuf, f64)>,
    pub scroll: f32,
    rows: Vec<Row>,
    rows_key: Option<String>,
    listings: HashMap<PathBuf, Arc<Vec<Listing>>>,
    voices: HashMap<PathBuf, Arc<Vec<(usize, String)>>>,
    info: HashMap<PathBuf, FileInfo>,
    asked: HashSet<(PathBuf, bool)>,
    reader: Option<Reader>,
    /// Bumped when the tree must be listed again (a folder opened, refreshed).
    generation: u64,
    /// The results and a selection changed this frame.
    pub selection_changed: bool,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            mode: ResultsMode::List,
            search: String::new(),
            category: Category::All,
            place: None,
            open: HashSet::new(),
            selected: Vec::new(),
            anchor: None,
            tag_filter: Vec::new(),
            focus_search: false,
            search_focused: false,
            stop_preview: false,
            new_folder: None,
            drag: None,
            previewing: None,
            pending_preview: None,
            scroll: 0.0,
            rows: Vec::new(),
            rows_key: None,
            listings: HashMap::new(),
            voices: HashMap::new(),
            info: HashMap::new(),
            asked: HashSet::new(),
            reader: None,
            generation: 0,
            selection_changed: false,
        }
    }
}

fn item_kind(found: &Found) -> ItemKind {
    match found.kind {
        "sample" => ItemKind::Sample,
        "midi" => ItemKind::Midi,
        "preset" => ItemKind::Preset,
        "project" => ItemKind::Project,
        "instrument" => ItemKind::Instrument,
        "effect" => ItemKind::Effect,
        _ => ItemKind::Heading,
    }
}

fn file_kind_item(kind: FileKind) -> ItemKind {
    match kind {
        FileKind::Sample | FileKind::Stems => ItemKind::Sample,
        FileKind::Midi => ItemKind::Midi,
        FileKind::Preset => ItemKind::Preset,
        FileKind::Project => ItemKind::Project,
    }
}

fn heading(key: &str, name: &str) -> BrowserItem {
    BrowserItem { key: key.into(), name: name.into(), kind: ItemKind::Heading, plugin: false }
}

impl BrowserState {
    /// The rows as last built.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Forgets what was listed, so folders are read again (after a refresh or a change).
    pub fn relist(&mut self) {
        self.listings.clear();
        self.voices.clear();
        self.generation = self.generation.wrapping_add(1);
        self.rows_key = None;
    }

    /// Opens or closes a node of the tree.
    pub fn toggle(&mut self, node: &str) {
        if !self.open.remove(node) {
            self.open.insert(node.to_string());
        }
        self.rows_key = None;
    }

    /// Selects `items`, replacing the selection (for a view that is not the results list, such
    /// as another way of showing the files).
    pub fn select(&mut self, items: Vec<BrowserItem>) {
        self.selected = items;
        self.selection_changed = true;
    }

    /// The selected items.
    pub fn selected(&self) -> &[BrowserItem] {
        &self.selected
    }

    /// Starts dragging `items` into the project, as dragging rows of the results does.
    pub fn start_drag(&mut self, items: Vec<BrowserItem>) {
        if !items.is_empty() {
            self.drag = Some(items);
        }
    }

    /// Builds the rows when what they come from changed.
    pub fn refresh_rows(&mut self, projects: &mut Projects) -> Result<(), String> {
        let revision = projects.file_library_mut().revision();
        let key = format!(
            "{}\u{1}{:?}\u{1}{:?}\u{1}{:?}\u{1}{revision}\u{1}{}",
            self.search, self.category, self.place, self.tag_filter, self.generation
        );
        if self.rows_key.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        self.rows_key = Some(key);
        let flat = !self.search.trim().is_empty()
            || !self.tag_filter.is_empty()
            || matches!(self.place, Some(Place::Pinned | Place::Recent(_)));
        self.rows = if flat { self.flat_rows(projects)? } else { self.tree_rows(projects) };
        // What is no longer listed is no longer selected.
        let listed: HashSet<&str> = self.rows.iter().map(|r| r.item.key.as_str()).collect();
        let before = self.selected.len();
        self.selected.retain(|s| listed.contains(s.key.as_str()));
        if before != self.selected.len() {
            self.selection_changed = true;
        }
        Ok(())
    }

    fn flat_rows(&mut self, projects: &mut Projects) -> Result<Vec<Row>, String> {
        let place = self.place.as_ref().map(Place::param);
        let found =
            suhina_session::search_items(projects, &self.search, self.category, place.as_deref(), &self.tag_filter, 0)
                .map_err(|e| e.0)?;
        Ok(found
            .into_iter()
            .map(|f| {
                let context = f.folder.as_ref().and_then(|folder| {
                    let path = match &f.item {
                        Item::File(p) => p.parent().map(Path::to_path_buf),
                        _ => None,
                    }?;
                    let below = path.strip_prefix(folder.parent().unwrap_or(folder)).ok()?;
                    Some(below.display().to_string())
                });
                let context = context.or_else(|| f.detail.clone());
                let item =
                    BrowserItem { key: f.item.key(), name: f.name.clone(), kind: item_kind(&f), plugin: f.plugin };
                Row { item, depth: 0, opens: false, open: false, context }
            })
            .collect())
    }

    fn tree_rows(&mut self, projects: &mut Projects) -> Vec<Row> {
        let mut rows = Vec::new();
        let category = self.category;
        match self.place.clone() {
            Some(Place::Folder(folder)) => self.folder_rows(projects, &folder, 0, &mut rows),
            _ => {
                let devices = matches!(category, Category::All | Category::Instruments | Category::Effects);
                if devices && category != Category::Effects {
                    self.device_rows("group:instruments", "Instruments", true, &mut rows);
                }
                if devices && category != Category::Instruments {
                    self.device_rows("group:effects", "Effects", false, &mut rows);
                }
                if matches!(category, Category::All | Category::Instruments | Category::Effects | Category::Plugins) {
                    self.plugin_rows(projects, &mut rows);
                }
                if !category.file_kinds().is_empty() {
                    let folders: Vec<PathBuf> =
                        projects.file_library_mut().folders().into_iter().map(|f| f.path).collect();
                    for folder in folders {
                        let name = folder
                            .file_name()
                            .map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into_owned());
                        let key = folder.display().to_string();
                        let open = self.open.contains(&key);
                        let item = BrowserItem { key: key.clone(), name, kind: ItemKind::Folder, plugin: false };
                        rows.push(Row { item, depth: 0, opens: true, open, context: None });
                        if open {
                            self.folder_rows(projects, &folder, 1, &mut rows);
                        }
                    }
                }
            }
        }
        rows
    }

    fn device_rows(&self, node: &str, name: &str, instruments: bool, rows: &mut Vec<Row>) {
        let open = self.open.contains(node);
        rows.push(Row { item: heading(node, name), depth: 0, opens: true, open, context: None });
        if !open {
            return;
        }
        for info in suhina_device_registry::DEVICES {
            let is_instrument = info.role == suhina_device_api::Role::Instrument;
            if is_instrument != instruments {
                continue;
            }
            let kind = if is_instrument { ItemKind::Instrument } else { ItemKind::Effect };
            let item = BrowserItem { key: format!("device:{}", info.id), name: info.name.into(), kind, plugin: false };
            rows.push(Row { item, depth: 1, opens: false, open: false, context: None });
        }
    }

    fn plugin_rows(&self, projects: &mut Projects, rows: &mut Vec<Row>) {
        let env = projects.plugin_env();
        let wanted = |instrument: bool| match self.category {
            Category::Instruments => instrument,
            Category::Effects => !instrument,
            _ => true,
        };
        let mut vendors: Vec<(String, Vec<BrowserItem>)> = Vec::new();
        for plugin in env.catalog.plugins() {
            let instrument = suhina_session::is_instrument(plugin);
            if !wanted(instrument) {
                continue;
            }
            let vendor = if plugin.vendor.trim().is_empty() {
                "Unknown vendor".to_string()
            } else {
                plugin.vendor.trim().to_string()
            };
            let kind = if instrument { ItemKind::Instrument } else { ItemKind::Effect };
            let item =
                BrowserItem { key: format!("plugin:{}", plugin.spec()), name: plugin.name.clone(), kind, plugin: true };
            match vendors.iter_mut().find(|(v, _)| *v == vendor) {
                Some((_, list)) => list.push(item),
                None => vendors.push((vendor, vec![item])),
            }
        }
        vendors.sort_by(|a, b| {
            (a.0 == "Unknown vendor").cmp(&(b.0 == "Unknown vendor")).then(suhina_library::natural_cmp(&a.0, &b.0))
        });
        let node = "group:plugins";
        let open = self.open.contains(node);
        rows.push(Row { item: heading(node, "Plug-ins"), depth: 0, opens: true, open, context: None });
        if !open {
            return;
        }
        for (vendor, mut list) in vendors {
            let key = format!("vendor:{vendor}");
            let vendor_open = self.open.contains(&key);
            rows.push(Row { item: heading(&key, &vendor), depth: 1, opens: true, open: vendor_open, context: None });
            if vendor_open {
                list.sort_by(|a, b| suhina_library::natural_cmp(&a.name, &b.name));
                for item in list {
                    rows.push(Row { item, depth: 2, opens: false, open: false, context: None });
                }
            }
        }
    }

    fn folder_rows(&mut self, projects: &mut Projects, folder: &Path, depth: usize, rows: &mut Vec<Row>) {
        if depth > suhina_library::index::MAX_DEPTH {
            return;
        }
        let listing = match self.listings.get(folder) {
            Some(listing) => Arc::clone(listing),
            None => {
                let listing = Arc::new(projects.file_library_mut().list(folder).unwrap_or_default());
                self.listings.insert(folder.to_path_buf(), Arc::clone(&listing));
                listing
            }
        };
        let kinds = self.category.file_kinds();
        for entry in listing.iter() {
            let key = entry.path.display().to_string();
            match entry.kind {
                None => {
                    // A folder the index shows holds nothing of the category is left out.
                    if self.category != Category::All
                        && projects.file_library_mut().holds(&entry.path, kinds) == Some(false)
                    {
                        continue;
                    }
                    let open = self.open.contains(&key);
                    let item = BrowserItem {
                        key: key.clone(),
                        name: entry.name.clone(),
                        kind: ItemKind::Folder,
                        plugin: false,
                    };
                    rows.push(Row { item, depth, opens: true, open, context: None });
                    if open {
                        self.folder_rows(projects, &entry.path, depth + 1, rows);
                    }
                }
                Some(kind) if kinds.contains(&kind) => {
                    let bank = kind == FileKind::Preset && self.preset_voices(&entry.path).len() > 1;
                    let open = bank && self.open.contains(&key);
                    let item = BrowserItem {
                        key: key.clone(),
                        name: entry.name.clone(),
                        kind: file_kind_item(kind),
                        plugin: false,
                    };
                    rows.push(Row { item, depth, opens: bank, open, context: None });
                    if open {
                        for (voice, name) in self.preset_voices(&entry.path).iter() {
                            let item = BrowserItem {
                                key: format!("{key}#{voice}"),
                                name: format!("{voice}  {name}"),
                                kind: ItemKind::Preset,
                                plugin: false,
                            };
                            rows.push(Row { item, depth: depth + 1, opens: false, open: false, context: None });
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }

    /// The voices of a preset file (read once; at most 64 KiB).
    pub fn preset_voices(&mut self, path: &Path) -> Arc<Vec<(usize, String)>> {
        if let Some(voices) = self.voices.get(path) {
            return Arc::clone(voices);
        }
        let voices = std::fs::metadata(path)
            .ok()
            .filter(|m| m.len() <= suhina_device_registry::MAX_PRESET_BYTES as u64)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|data| suhina_device_registry::preset_voices(&data).ok())
            .unwrap_or_default();
        let voices = Arc::new(voices);
        self.voices.insert(path.to_path_buf(), Arc::clone(&voices));
        voices
    }

    /// What is known about a file; asks the reader for it when it is not known yet. `whole`
    /// asks for the decoded sample and its waveform too.
    pub fn info(&mut self, path: &Path, whole: bool) -> Option<&FileInfo> {
        let known =
            self.info.get(path).is_some_and(|i| !whole || i.data.is_some() || i.notes.is_some() || i.error.is_some());
        if !known && self.asked.insert((path.to_path_buf(), whole)) {
            let reader = self.reader.get_or_insert_with(Reader::start);
            if let Some(to) = &reader.to {
                let request =
                    if whole { Request::Whole(path.to_path_buf()) } else { Request::Header(path.to_path_buf()) };
                let _ = to.send(request);
            }
        }
        self.info.get(path)
    }

    /// Takes in what the reader has read. Returns whether anything arrived.
    pub fn poll(&mut self) -> bool {
        let Some(from) = self.reader.as_ref().and_then(|r| r.from.as_ref()) else { return false };
        let mut any = false;
        while let Ok(answer) = from.try_recv() {
            any = true;
            self.asked.remove(&(answer.path.clone(), answer.whole));
            if self.info.len() >= MAX_INFO {
                self.info.clear();
            }
            // Decoded audio is kept only for the selected file.
            let selected = self.selected.iter().any(|s| s.path().as_deref() == Some(answer.path.as_path()));
            let mut info = answer.info;
            if !selected {
                info.data = None;
            }
            match self.info.get_mut(&answer.path) {
                Some(known) if !answer.whole => {
                    known.seconds = known.seconds.or(info.seconds);
                    known.key = known.key.clone().or(info.key);
                    known.tempo = known.tempo.or(info.tempo);
                }
                _ => {
                    self.info.insert(answer.path, info);
                }
            }
        }
        any
    }

    /// Drops the decoded audio of files no longer selected.
    pub fn forget_unselected(&mut self) {
        let selected: HashSet<PathBuf> = self.selected.iter().filter_map(BrowserItem::path).collect();
        for (path, info) in &mut self.info {
            if !selected.contains(path) {
                info.data = None;
            }
        }
    }

    /// Asks for `path` to preview from `from` seconds once it is decoded.
    pub fn want_preview(&mut self, path: PathBuf, from: f64) {
        self.pending_preview = Some((path, from));
    }

    /// A preview whose sample is decoded now, taken once.
    pub fn take_ready_preview(&mut self) -> Option<(PathBuf, f64, Arc<AudioData>)> {
        let (path, from) = self.pending_preview.clone()?;
        let info = self.info.get(&path)?;
        if info.error.is_some() {
            self.pending_preview = None;
            return None;
        }
        let data = Arc::clone(info.data.as_ref()?);
        self.pending_preview = None;
        Some((path, from, data))
    }

    pub fn cancel_preview(&mut self) {
        self.pending_preview = None;
        self.previewing = None;
    }

    /// Whether a sample waits to be decoded before it previews.
    pub fn preview_pending(&self) -> bool {
        self.pending_preview.is_some()
    }
}

/// Text for a length in seconds: `0:04.2`, `3:21`.
pub fn duration_text(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return String::new();
    }
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    if seconds < 60.0 { format!("0:{rest:04.1}") } else { format!("{}:{:02}", minutes as u64, rest.floor() as u64) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(duration_text(4.2), "0:04.2");
        assert_eq!(duration_text(0.06), "0:00.1");
        assert_eq!(duration_text(201.7), "3:21");
        assert_eq!(duration_text(f64::NAN), "");
    }
}
