//! Backups (autosave) of the open projects: when they are written, where, the thread that writes
//! them, clean-up, and the journal the offer on start reads (`docs/spec/backups.md`, ADR 0010
//! decision 11). The window's thread only copies the project; turning it into JSON and writing
//! it happens on the backup thread, and nothing here is reachable from the audio thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use suhina_model::Project;

use crate::commands::{CommandKind, CommandSpec, ParamType, optional};
use crate::{CommandError, ProjectId, Projects};

/// When backups are written (Settings › General › Backups, `project.autosave`'s `interval`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interval {
    /// Just before each critical operation ([`CRITICAL_COMMANDS`], closing a project, quitting).
    Critical,
    /// Every so many minutes.
    Minutes(u32),
    Never,
}

impl Default for Interval {
    fn default() -> Self {
        Self::Minutes(5)
    }
}

impl Interval {
    /// Every choice, in the order Settings lists them.
    pub const ALL: [Interval; 9] = [
        Self::Critical,
        Self::Minutes(1),
        Self::Minutes(5),
        Self::Minutes(10),
        Self::Minutes(15),
        Self::Minutes(30),
        Self::Minutes(60),
        Self::Minutes(240),
        Self::Never,
    ];

    /// The name the settings file and `project.autosave` use: `critical`, `5m`, `4h`, `never`.
    pub fn name(self) -> String {
        match self {
            Self::Critical => "critical".into(),
            Self::Never => "never".into(),
            Self::Minutes(m) if m >= 60 && m % 60 == 0 && m != 60 => format!("{}h", m / 60),
            Self::Minutes(m) => format!("{m}m"),
        }
    }

    /// What Settings shows for it.
    pub fn title(self) -> String {
        match self {
            Self::Critical => "Before critical operations".into(),
            Self::Never => "Never".into(),
            Self::Minutes(1) => "Every minute".into(),
            Self::Minutes(m) if m >= 120 && m % 60 == 0 => format!("Every {} hours", m / 60),
            Self::Minutes(m) => format!("Every {m} minutes"),
        }
    }

    /// One of [`Interval::ALL`] by its [`Interval::name`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.name() == text.trim())
    }

    /// How often a timed backup is written; `None` for the others.
    pub fn period(self) -> Option<Duration> {
        match self {
            Self::Minutes(m) => Some(Duration::from_secs(u64::from(m).saturating_mul(60))),
            _ => None,
        }
    }
}

/// Commands after which the edits before them are hard to get back: with
/// [`Interval::Critical`] a backup is written just before each. Closing a project and quitting
/// are critical too ([`Projects::close`], [`Projects::finish_backups`]).
pub const CRITICAL_COMMANDS: &[&str] =
    &["projects.close", "project.relinkFiles", "clip.addFiles", "device.addPlugin", "track.delete"];

/// Backups of one project that clean-up keeps.
pub const KEEP: usize = 20;
/// The folder beside a project file that its backups go to.
pub const FOLDER: &str = ".backup";
/// The folder in Suhina's settings folder for backups of projects never saved, and the journal.
pub const SETTINGS_SUBFOLDER: &str = "backups";
const JOURNAL: &str = "journal.json";
const JOURNAL_FORMAT: &str = "suhina-backups";
/// The journal is read only up to this size and this many entries.
const MAX_JOURNAL: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 256;
/// Files a backup folder is looked through for clean-up, at most.
const MAX_LISTED: usize = 10_000;
/// Failures kept for the status bar until the window takes them.
const MAX_NOTICES: usize = 8;
/// How long `project.autosave` with `now` waits for its backup.
const NOW_WAIT: Duration = Duration::from_secs(10);

/// A backup that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    pub written: SystemTime,
}

/// A project's backup state, kept in its session.
#[derive(Debug, Clone)]
pub(crate) struct SessionBackups {
    /// The session's change count when it was last backed up.
    pub(crate) backed_at: u64,
    /// When the interval last started: the project opened or last backed up.
    pub(crate) since: Instant,
    /// Backups written to the settings folder, while the project had no file of its own.
    pub(crate) in_settings: Vec<PathBuf>,
    pub(crate) last: Option<Backup>,
}

impl Default for SessionBackups {
    fn default() -> Self {
        Self { backed_at: 0, since: Instant::now(), in_settings: Vec::new(), last: None }
    }
}

/// A backup a run of Suhina left in the journal that is newer than its project file: what the
/// window offers on start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    pub backup: PathBuf,
    /// The project file it is a backup of; `None` for a project never saved.
    pub file: Option<PathBuf>,
    pub written: SystemTime,
}

impl Leftover {
    /// The project's name: its file's, or the one in the backup's name.
    pub fn name(&self) -> String {
        if let Some(stem) = self.file.as_deref().and_then(Path::file_stem) {
            return stem.to_string_lossy().into_owned();
        }
        let stem = self.backup.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        project_name_of(&stem).unwrap_or(stem)
    }
}

/// `written` as local date and time, `2026-10-15 14:05:09`.
pub fn date_time(written: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(written).format("%Y-%m-%d %H:%M:%S").to_string()
}

/// `written` as the local time of day, `14:05`.
pub fn clock_time(written: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(written).format("%H:%M").to_string()
}

/// The settings and the thread the backups of [`Projects`] use.
pub(crate) struct Backups {
    pub(crate) interval: Interval,
    pub(crate) clean_up: bool,
    /// `backups` in Suhina's settings folder: backups of projects never saved, and the journal.
    folder: Option<PathBuf>,
    /// Tells this run's journal entries from other runs'.
    run: String,
    writer: Option<Writer>,
    notices: Vec<String>,
}

impl Default for Backups {
    fn default() -> Self {
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Self {
            interval: Interval::default(),
            clean_up: true,
            folder: None,
            run: format!("{nanos}-{}", std::process::id()),
            writer: None,
            notices: Vec::new(),
        }
    }
}

impl Backups {
    /// Keeps a failure for the status bar; only the latest few are kept.
    fn note(&mut self, notice: String) {
        if self.notices.len() >= MAX_NOTICES {
            self.notices.remove(0);
        }
        self.notices.push(notice);
    }

    fn journal(&self) -> Option<PathBuf> {
        self.folder.as_ref().map(|f| f.join(JOURNAL))
    }

    fn send(&mut self, message: Message) {
        if self.writer.is_none() {
            match Writer::start(self.journal()) {
                Ok(writer) => self.writer = Some(writer),
                Err(e) => {
                    self.note(format!("backups cannot be written: {e}"));
                    return;
                }
            }
        }
        if let Some(writer) = &self.writer
            && writer.to.send(message).is_err()
        {
            self.writer = None;
            self.note("the backup thread stopped; backups start again with the next one".into());
        }
    }

    /// Waits up to `timeout` until the backup thread has done everything sent to it.
    fn flush(&mut self, timeout: Duration) -> bool {
        let (done, wait) = mpsc::channel();
        self.send(Message::Flush(done));
        wait.recv_timeout(timeout).is_ok()
    }
}

/// What the window learns from the backups each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tick {
    /// Backups that failed, for the status bar.
    pub notices: Vec<String>,
    /// When to look again, for a timed backup that is waiting.
    pub next: Option<Duration>,
}

enum Message {
    Write(Box<Job>),
    /// Takes this run's entry for a project out of the journal (closed, saved).
    Forget {
        run: String,
        project: u32,
    },
    /// Takes the entry for this backup out of the journal (opened or ignored on start).
    ForgetBackup(PathBuf),
    /// Takes the entries of other runs whose backups are no longer worth offering out.
    Prune {
        run: String,
    },
    /// Deletes these backups (a project never saved, once it is).
    Delete(Vec<PathBuf>),
    Flush(mpsc::Sender<()>),
}

struct Job {
    run: String,
    project: u32,
    file: Option<PathBuf>,
    name: String,
    folder: PathBuf,
    snapshot: Project,
    clean_up: bool,
}

/// A backup the thread finished, or why it could not.
struct Done {
    project: u32,
    result: Result<Backup, String>,
    in_settings: bool,
}

struct Writer {
    to: mpsc::Sender<Message>,
    from: mpsc::Receiver<Done>,
}

impl Writer {
    fn start(journal: Option<PathBuf>) -> std::io::Result<Self> {
        let (to, messages) = mpsc::channel::<Message>();
        let (done, from) = mpsc::channel::<Done>();
        std::thread::Builder::new().name("suhina-backup".into()).spawn(move || {
            let mut entries = journal.as_deref().map(read_journal).unwrap_or_default();
            for message in messages {
                match message {
                    Message::Write(job) => {
                        let result = write_backup(&job);
                        if let (Ok(backup), Some(_)) = (&result, &journal) {
                            entries.retain(|e| !(e.run == job.run && e.project == job.project));
                            entries.push(Entry {
                                run: job.run.clone(),
                                project: job.project,
                                file: job.file.clone(),
                                backup: backup.path.clone(),
                            });
                            if entries.len() > MAX_ENTRIES {
                                entries.remove(0);
                            }
                            save_journal(journal.as_deref(), &entries);
                        }
                        let in_settings = job.file.is_none();
                        let _gone = done.send(Done { project: job.project, result, in_settings });
                    }
                    Message::Forget { run, project } => {
                        let before = entries.len();
                        entries.retain(|e| !(e.run == run && e.project == project));
                        if entries.len() != before {
                            save_journal(journal.as_deref(), &entries);
                        }
                    }
                    Message::ForgetBackup(path) => {
                        let before = entries.len();
                        entries.retain(|e| e.backup != path);
                        if entries.len() != before {
                            save_journal(journal.as_deref(), &entries);
                        }
                    }
                    Message::Prune { run } => {
                        let before = entries.len();
                        entries.retain(|e| e.run == run || offered(e).is_some());
                        if entries.len() != before {
                            save_journal(journal.as_deref(), &entries);
                        }
                    }
                    Message::Delete(paths) => {
                        for path in paths {
                            let _gone = std::fs::remove_file(path);
                        }
                    }
                    Message::Flush(reply) => {
                        let _gone = reply.send(());
                    }
                }
            }
        })?;
        Ok(Self { to, from })
    }
}

/// One line of the journal: a project's newest backup in one run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    run: String,
    project: u32,
    file: Option<PathBuf>,
    backup: PathBuf,
}

/// The journal's entries; nothing when it is missing, too large or not a journal.
fn read_journal(path: &Path) -> Vec<Entry> {
    let too_large = std::fs::metadata(path).map_or(true, |m| m.len() > MAX_JOURNAL);
    if too_large {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    parse_journal(&text)
}

fn parse_journal(text: &str) -> Vec<Entry> {
    let Ok(root) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    if root.get("format").and_then(Value::as_str) != Some(JOURNAL_FORMAT) {
        return Vec::new();
    }
    let Some(list) = root.get("backups").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .take(MAX_ENTRIES)
        .filter_map(|e| {
            Some(Entry {
                run: e.get("run")?.as_str()?.to_string(),
                project: u32::try_from(e.get("project")?.as_u64()?).ok()?,
                file: e.get("file").and_then(Value::as_str).map(PathBuf::from),
                backup: PathBuf::from(e.get("backup")?.as_str()?),
            })
        })
        .collect()
}

fn journal_text(entries: &[Entry]) -> String {
    let list: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "run": e.run,
                "project": e.project,
                "file": e.file.as_ref().map(|f| f.display().to_string()),
                "backup": e.backup.display().to_string(),
            })
        })
        .collect();
    let root = json!({ "format": JOURNAL_FORMAT, "version": 1, "backups": list });
    serde_json::to_string_pretty(&root).unwrap_or_default()
}

/// Writes the journal through a temporary file; a journal that cannot be written only means a
/// crash is not offered back, so it is not an error.
fn save_journal(path: Option<&Path>, entries: &[Entry]) {
    if let Some(path) = path {
        let _unwritten = write_atomically(path, journal_text(entries).as_bytes());
    }
}

/// `text` to a temporary file beside `path`, which is then renamed to it.
fn write_atomically(path: &Path, text: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".saving");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _gone = std::fs::remove_file(&temporary);
    })
}

/// The entry as an offer, when its backup is there and newer than its project file (or the
/// project has no file, or it is gone).
fn offered(entry: &Entry) -> Option<Leftover> {
    let written = std::fs::metadata(&entry.backup).and_then(|m| m.modified()).ok()?;
    let file_time = entry.file.as_ref().and_then(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok());
    if file_time.is_some_and(|saved| saved >= written) {
        return None;
    }
    Some(Leftover { backup: entry.backup.clone(), file: entry.file.clone(), written })
}

/// Writes one backup: picks its name, writes it through a temporary file, then cleans up.
fn write_backup(job: &Job) -> Result<Backup, String> {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
    let base = format!("{} {stamp}", job.name);
    let path = (1..=99u32)
        .map(|n| {
            let name = if n == 1 { base.clone() } else { format!("{base} ({n})") };
            job.folder.join(format!("{name}.{}", suhina_project_file::EXTENSION))
        })
        .find(|p| !p.exists())
        .ok_or_else(|| format!("{}: too many backups in the same second", job.folder.display()))?;
    #[cfg(test)]
    if let Ok(mut names) = tests::WRITER_THREADS.lock() {
        names.push(std::thread::current().name().unwrap_or_default().to_string());
    }
    let text = suhina_project_file::to_json(&job.snapshot);
    write_atomically(&path, text.as_bytes())
        .map_err(|e| format!("could not write a backup to {}: {e}", path.display()))?;
    if job.clean_up {
        clean_up(&job.folder, &job.name, KEEP);
    }
    let written = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or_else(|_| SystemTime::now());
    Ok(Backup { path, written })
}

/// Deletes all but the newest `keep` backups of the project `name` in `folder`.
pub(crate) fn clean_up(folder: &Path, name: &str, keep: usize) {
    let Ok(listing) = std::fs::read_dir(folder) else { return };
    let mut ours: Vec<(SystemTime, PathBuf)> = listing
        .take(MAX_LISTED)
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().and_then(project_name_of_file).is_some_and(|n| n == name))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    ours.sort_by(|a, b| b.cmp(a));
    for (_, path) in ours.into_iter().skip(keep) {
        let _gone = std::fs::remove_file(path);
    }
}

/// The project name in a backup's file name (`Song 2026-10-15 14-05-09 (2).suhina` is
/// `Song`), or `None` when it is not a backup's name.
fn project_name_of_file(file: &str) -> Option<String> {
    let stem = file.strip_suffix(&format!(".{}", suhina_project_file::EXTENSION))?;
    project_name_of(stem)
}

fn project_name_of(stem: &str) -> Option<String> {
    // An optional " (n)" after the time.
    let stem = match stem.strip_suffix(')').and_then(|s| s.rsplit_once(" (")) {
        Some((before, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => before,
        _ => stem,
    };
    // "<name> YYYY-MM-DD HH-MM-SS": the last 20 characters are the space and the stamp.
    let split = stem.len().checked_sub(20)?;
    let (name, stamp) = (stem.get(..split)?, stem.get(split..)?);
    let pattern = " dddd-dd-dd dd-dd-dd";
    let fits = stamp.len() == pattern.len()
        && stamp.bytes().zip(pattern.bytes()).all(|(s, p)| if p == b'd' { s.is_ascii_digit() } else { s == p });
    (fits && !name.is_empty()).then(|| name.to_string())
}

/// A project name that can be part of a file name on every system.
fn file_name_part(name: &str) -> String {
    let cleaned: String =
        name.trim()
            .chars()
            .map(|c| {
                if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '_'
                } else {
                    c
                }
            })
            .take(100)
            .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if cleaned.is_empty() { "Untitled".into() } else { cleaned }
}

impl Projects {
    /// Uses `folder` (`backups` in Suhina's settings folder) for backups of projects never saved
    /// and for the journal the offer on start reads. Without one (the CLI), only projects with a
    /// file are backed up and nothing is offered on start.
    pub fn set_backup_folder(&mut self, folder: Option<PathBuf>) {
        if self.backups.folder != folder {
            self.backups.writer = None;
            self.backups.folder = folder;
        }
    }

    /// When backups are written, and whether old ones are cleaned up (the window's settings).
    pub fn set_backup_settings(&mut self, interval: Interval, clean_up: bool) {
        self.backups.interval = interval;
        self.backups.clean_up = clean_up;
    }

    pub fn backup_interval(&self) -> Interval {
        self.backups.interval
    }

    /// Where the backups of project `id` go: `.backup` beside the file it came from, or the
    /// settings folder's `backups` for one with no file.
    pub fn backup_folder(&self, id: ProjectId) -> Option<PathBuf> {
        let session = self.get(id)?;
        match session.source() {
            Some(source) => Some(source.parent().map_or_else(PathBuf::new, Path::to_path_buf).join(FOLDER)),
            None => self.backups.folder.clone(),
        }
    }

    /// Whether project `id` has changes that are neither saved nor backed up.
    pub fn backup_due(&self, id: ProjectId) -> bool {
        self.get(id).is_some_and(|s| s.has_unsaved_changes() && s.changes != s.backup.backed_at)
    }

    /// Sends a backup of project `id` to the backup thread when it has changes that are not
    /// backed up yet. Returns whether one was sent.
    pub fn back_up(&mut self, id: ProjectId) -> Result<bool, CommandError> {
        // What finished meanwhile, so nothing piles up where nobody ticks (the CLI, MCP).
        self.take_finished();
        if !self.backup_due(id) {
            return Ok(false);
        }
        let folder = self.backup_folder(id).ok_or("a project never saved has no folder to back up to here")?;
        let name = file_name_part(self.name(id).unwrap_or("Untitled"));
        let (clean_up, run) = (self.backups.clean_up, self.backups.run.clone());
        let session = self.get_mut(id).ok_or("no such project")?;
        let file = session.source().map(Path::to_path_buf);
        let snapshot = session.snapshot(&folder);
        session.backup.backed_at = session.changes;
        session.backup.since = Instant::now();
        self.backups.send(Message::Write(Box::new(Job { run, project: id.0, file, name, folder, snapshot, clean_up })));
        Ok(true)
    }

    /// Before command `id`: with [`Interval::Critical`], backs up the active project when `id`
    /// is one of the [`CRITICAL_COMMANDS`]. The window calls it before the commands it runs
    /// on the session directly; [`Projects::execute_in`] calls it for the rest.
    pub fn back_up_before(&mut self, id: &str) {
        if self.backups.interval == Interval::Critical
            && CRITICAL_COMMANDS.contains(&id)
            && let Some(active) = self.active()
        {
            self.back_up_noting(active);
        }
    }

    /// A critical backup of project `id`, its failure kept for the status bar.
    pub(crate) fn back_up_critical(&mut self, id: ProjectId) {
        if self.backups.interval == Interval::Critical {
            self.back_up_noting(id);
        }
    }

    fn back_up_noting(&mut self, id: ProjectId) {
        if let Err(e) = self.back_up(id) {
            self.backups.note(e.0);
        }
    }

    /// Takes in what the backup thread finished and sends the timed backups that are due. Call
    /// it often from the thread that owns the projects; the window does every frame.
    pub fn tick_backups(&mut self) -> Tick {
        self.take_finished();
        let mut next = None;
        if let Some(period) = self.backups.interval.period() {
            for id in self.ids() {
                if !self.backup_due(id) {
                    continue;
                }
                let waited = self.get(id).map_or(Duration::ZERO, |s| s.backup.since.elapsed());
                match period.checked_sub(waited) {
                    Some(left) if !left.is_zero() => {
                        next = Some(next.map_or(left, |n: Duration| n.min(left)));
                    }
                    _ => self.back_up_noting(id),
                }
            }
        }
        Tick { notices: std::mem::take(&mut self.backups.notices), next }
    }

    fn take_finished(&mut self) {
        let finished: Vec<Done> = match &self.backups.writer {
            Some(writer) => writer.from.try_iter().collect(),
            None => Vec::new(),
        };
        for done in finished {
            match done.result {
                Ok(backup) => {
                    if let Some(session) = self.get_mut(ProjectId(done.project)) {
                        if done.in_settings && session.backup.in_settings.len() < MAX_ENTRIES {
                            session.backup.in_settings.push(backup.path.clone());
                        }
                        session.backup.last = Some(backup);
                    }
                }
                Err(e) => self.backups.note(e),
            }
        }
    }

    /// After project `id` was saved to a file of its own: its journal entry goes, and so do the
    /// backups written for it in the settings folder while it had no file.
    pub(crate) fn backups_after_save(&mut self, id: ProjectId) {
        let run = self.backups.run.clone();
        self.backups.send(Message::Forget { run, project: id.0 });
        let written = self.get_mut(id).map(|s| std::mem::take(&mut s.backup.in_settings)).unwrap_or_default();
        if !written.is_empty() {
            self.backups.send(Message::Delete(written));
        }
    }

    /// Project `id` is closing: its journal entry goes, as it is no longer open to lose.
    pub(crate) fn backups_after_close(&mut self, id: ProjectId) {
        let run = self.backups.run.clone();
        self.backups.send(Message::Forget { run, project: id.0 });
    }

    /// Suhina is quitting: with [`Interval::Critical`] each project with unsaved changes is
    /// backed up, every open project's journal entry goes, and the backup thread is given up
    /// to `timeout` to finish. Returns whether it did.
    pub fn finish_backups(&mut self, timeout: Duration) -> bool {
        for id in self.ids() {
            self.back_up_critical(id);
            self.backups_after_close(id);
        }
        if self.backups.writer.is_none() {
            return true;
        }
        self.backups.flush(timeout)
    }

    /// The backups left in the journal by a run that did not end normally and newer than their
    /// project files, oldest first, to offer on start. Entries not worth offering are taken
    /// out of the journal.
    pub fn leftover_backups(&mut self) -> Vec<Leftover> {
        let Some(journal) = self.backups.journal() else { return Vec::new() };
        let run = self.backups.run.clone();
        let mut found: Vec<Leftover> =
            read_journal(&journal).iter().filter(|e| e.run != run).filter_map(offered).collect();
        found.sort_by_key(|l| l.written);
        found.dedup_by(|a, b| a.backup == b.backup);
        self.backups.send(Message::Prune { run });
        found
    }

    /// Takes a backup offered on start out of the journal, once it was opened or ignored.
    pub fn forget_backup(&mut self, backup: &Path) {
        self.backups.send(Message::ForgetBackup(backup.to_path_buf()));
    }
}

/// The `project.autosave` command.
pub(crate) fn backup_commands() -> &'static [CommandSpec<Projects>] {
    BACKUP_COMMANDS
}

static BACKUP_COMMANDS: &[CommandSpec<Projects>] = &[CommandSpec {
    id: "project.autosave",
    label: "Report the active project's backups (where they go, the last one, whether one is due) and the backup settings; now writes a backup at once, interval and clean_up set when they are written",
    params: &[
        optional("now", ParamType::Bool, "true writes a backup of the active project now, if it has unsaved changes"),
        optional(
            "interval",
            ParamType::Text,
            "when backups are written: critical, 1m, 5m, 10m, 15m, 30m, 60m, 4h or never",
        ),
        optional("clean_up", ParamType::Bool, "true keeps only the newest 20 backups of each project"),
    ],
    kind: CommandKind::Projects,
    run: autosave,
}];

fn autosave(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let flag = |name: &str| match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(CommandError(format!("{name} must be true or false"))),
    };
    let interval = match params.get("interval") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(Interval::parse(text).ok_or_else(|| {
            let names: Vec<String> = Interval::ALL.iter().map(|i| i.name()).collect();
            CommandError(format!("interval must be one of {}", names.join(", ")))
        })?),
        Some(_) => return Err("interval must be text".into()),
    };
    let (now, clean_up) = (flag("now")?, flag("clean_up")?);
    if let Some(interval) = interval {
        projects.backups.interval = interval;
    }
    if let Some(clean_up) = clean_up {
        projects.backups.clean_up = clean_up;
    }
    let mut written = Value::Null;
    if now == Some(true) {
        let id = projects.active().ok_or("no project is open")?;
        if projects.back_up(id)? {
            if !projects.backups.flush(NOW_WAIT) {
                return Err("the backup did not finish in time".into());
            }
            projects.take_finished();
            if let Some(failed) = projects.backups.notices.pop() {
                return Err(CommandError(failed));
            }
            written = projects
                .get(id)
                .and_then(|s| s.backup.last.as_ref())
                .map_or(Value::Null, |b| json!(b.path.display().to_string()));
        }
    } else {
        projects.take_finished();
    }
    let active = projects.active();
    let last = active.and_then(|id| projects.get(id)).and_then(|s| s.backup.last.clone());
    let mut result = json!({
        "interval": projects.backups.interval.name(),
        "clean_up": projects.backups.clean_up,
        "folder": active.and_then(|id| projects.backup_folder(id)).map(|f| f.display().to_string()),
        "last_backup": last.map(|b| json!({ "path": b.path.display().to_string(), "written": date_time(b.written) })),
        "due": active.is_some_and(|id| projects.backup_due(id)),
    });
    if now == Some(true) {
        result["written"] = written;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Session, blank_project};
    use serde_json::json;

    /// The threads backups were written on.
    pub(super) static WRITER_THREADS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    /// Projects whose backups and journal go to a folder of their own, with one blank project
    /// open, changed once.
    fn changed_projects(settings: &Path) -> (Projects, ProjectId) {
        let mut projects = Projects::new();
        projects.set_backup_folder(Some(settings.to_path_buf()));
        let id = projects.add(Session::new(blank_project()), None).unwrap();
        projects.execute("transport.setTempo", &json!({ "bpm": 133 })).unwrap();
        (projects, id)
    }

    fn files_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    fn wait_for_backup(projects: &mut Projects, id: ProjectId) -> Backup {
        assert!(projects.backups.flush(Duration::from_secs(10)));
        projects.tick_backups();
        projects.get(id).and_then(|s| s.last_backup().cloned()).expect("a backup was written")
    }

    #[test]
    fn unsaved_changes_are_backed_up_once_the_interval_has_passed() {
        let settings = temporary("interval");
        let (mut projects, id) = changed_projects(&settings);
        projects.set_backup_settings(Interval::Minutes(1), true);
        let tick = projects.tick_backups();
        assert!(tick.next.is_some_and(|n| n <= Duration::from_secs(60)), "{tick:?}");
        assert!(projects.get(id).unwrap().last_backup().is_none(), "not before the interval");
        // A minute later.
        projects.get_mut(id).unwrap().backup.since = Instant::now() - Duration::from_secs(61);
        let tick = projects.tick_backups();
        assert!(tick.notices.is_empty(), "{tick:?}");
        let backup = wait_for_backup(&mut projects, id);
        assert_eq!(backup.path.parent(), Some(settings.as_path()), "a project never saved backs up to settings");
        assert_eq!(
            project_name_of_file(&backup.path.file_name().unwrap().to_string_lossy()).as_deref(),
            Some("Untitled 1")
        );
        let copy = suhina_project_file::read_file(&backup.path).unwrap();
        assert_eq!(copy.transport.tempo_bpm, 133.0);
        // Nothing changed since: no second backup, however long it waits.
        projects.get_mut(id).unwrap().backup.since = Instant::now() - Duration::from_secs(600);
        assert_eq!(projects.tick_backups().next, None);
        assert!(!projects.backup_due(id));
        assert!(projects.backups.flush(Duration::from_secs(10)));
        assert_eq!(files_in(&settings).iter().filter(|n| n.ends_with(".suhina")).count(), 1);
        // The backups were written on the backup thread, never on this one.
        let threads = WRITER_THREADS.lock().unwrap().clone();
        assert!(!threads.is_empty() && threads.iter().all(|t| t == "suhina-backup"), "{threads:?}");
        let _ = std::fs::remove_dir_all(settings);
    }

    #[test]
    fn never_and_critical_write_nothing_on_a_timer() {
        let settings = temporary("never");
        let (mut projects, id) = changed_projects(&settings);
        for interval in [Interval::Never, Interval::Critical] {
            projects.set_backup_settings(interval, true);
            projects.get_mut(id).unwrap().backup.since = Instant::now() - Duration::from_secs(100_000);
            assert_eq!(projects.tick_backups(), Tick::default());
        }
        assert!(projects.backups.writer.is_none(), "nothing was sent");
        let _ = std::fs::remove_dir_all(settings);
    }

    #[test]
    fn a_saved_project_backs_up_beside_its_file_and_a_save_takes_the_offer_away() {
        let settings = temporary("save-settings");
        let music = temporary("save-music");
        let (mut projects, id) = changed_projects(&settings);
        projects.set_backup_settings(Interval::Critical, true);
        // A backup while it had no file, in the settings folder.
        let first = projects.execute("project.autosave", &json!({ "now": true })).unwrap();
        let in_settings = PathBuf::from(first["written"].as_str().unwrap());
        assert!(in_settings.starts_with(&settings));
        let file = music.join("Song.suhina");
        projects.execute("project.save", &json!({ "path": file.display().to_string() })).unwrap();
        assert!(projects.backups.flush(Duration::from_secs(10)));
        assert!(!in_settings.exists(), "the save took away the copy in the settings folder");
        assert!(read_journal(&settings.join(JOURNAL)).is_empty(), "nothing to offer after a save");
        // Saved, so deleting a track needs no backup before it.
        projects.execute("track.delete", &json!({ "track": 1 })).unwrap();
        assert!(projects.backups.flush(Duration::from_secs(10)));
        assert!(!music.join(FOLDER).exists());
        projects.execute("edit.undo", &json!({})).unwrap();
        projects.execute("project.save", &json!({})).unwrap();
        // Changed: deleting a track is critical, so a backup goes to .backup first.
        projects.execute("transport.setTempo", &json!({ "bpm": 120 })).unwrap();
        projects.execute("track.delete", &json!({ "track": 1 })).unwrap();
        let backup = wait_for_backup(&mut projects, id);
        assert_eq!(backup.path.parent(), Some(music.join(FOLDER).as_path()));
        assert!(project_name_of_file(&backup.path.file_name().unwrap().to_string_lossy()).is_some_and(|n| n == "Song"));
        assert_eq!(suhina_project_file::read_file(&backup.path).unwrap().tracks.len(), blank_project().tracks.len());
        assert_eq!(read_journal(&settings.join(JOURNAL)).len(), 1);
        let info = projects.execute("project.autosave", &json!({})).unwrap();
        assert_eq!(info["interval"], "critical");
        assert_eq!(info["folder"], music.join(FOLDER).display().to_string());
        assert_eq!(info["last_backup"]["path"], backup.path.display().to_string());
        assert_eq!(info["due"], true, "the delete itself is not backed up yet");
        // Saving again leaves the history in .backup but takes the journal entry away.
        projects.execute("project.save", &json!({})).unwrap();
        assert!(projects.backups.flush(Duration::from_secs(10)));
        assert!(backup.path.exists());
        assert!(read_journal(&settings.join(JOURNAL)).is_empty());
        let _ = std::fs::remove_dir_all(settings);
        let _ = std::fs::remove_dir_all(music);
    }

    #[test]
    fn a_backup_left_by_a_crash_is_offered_and_opens_unsaved_tied_to_its_file() {
        let settings = temporary("crash-settings");
        let music = temporary("crash-music");
        let file = music.join("Song.suhina");
        {
            let mut session = Session::new(blank_project());
            session.save(&file, false).unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
        let backup = {
            // The run that crashes: opened, changed, backed up, never closed.
            let mut projects = Projects::new();
            projects.set_backup_folder(Some(settings.clone()));
            let (session, _) = Session::open(&file).unwrap();
            let id = projects.add(session, None).unwrap();
            projects.execute("transport.setTempo", &json!({ "bpm": 99 })).unwrap();
            projects.execute("project.autosave", &json!({ "now": true })).unwrap();
            wait_for_backup(&mut projects, id).path
        };
        let mut next_run = Projects::new();
        next_run.set_backup_folder(Some(settings.clone()));
        let offers = next_run.leftover_backups();
        assert_eq!(offers.len(), 1, "{offers:?}");
        let offer = offers.first().unwrap();
        assert_eq!(
            (offer.backup.clone(), offer.file.clone(), offer.name()),
            (backup.clone(), Some(file.clone()), "Song".into())
        );
        let (session, warnings) = Session::recover(&offer.backup, offer.file.as_deref()).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(session.source(), Some(file.as_path()));
        assert!(session.has_unsaved_changes());
        assert_eq!(session.project().transport.tempo_bpm, 99.0);
        let id = next_run.add(session, None).unwrap();
        assert_eq!(next_run.name(id), Some("Song"));
        assert!(!next_run.backup_due(id), "the backup already holds it");
        next_run.forget_backup(&offer.backup);
        assert!(next_run.backups.flush(Duration::from_secs(10)));
        assert!(next_run.leftover_backups().is_empty(), "offered once");
        // Saving writes the original file, and the backup stays.
        next_run.execute("project.save", &json!({})).unwrap();
        assert_eq!(suhina_project_file::read_file(&file).unwrap().transport.tempo_bpm, 99.0);
        assert!(backup.exists());
        let _ = std::fs::remove_dir_all(settings);
        let _ = std::fs::remove_dir_all(music);
    }

    #[test]
    fn closing_and_quitting_normally_leave_nothing_to_offer() {
        let settings = temporary("quit");
        let (mut projects, id) = changed_projects(&settings);
        projects.set_backup_settings(Interval::Critical, true);
        // Closing is critical: the changes are backed up, then the entry goes.
        projects.execute("projects.close", &json!({ "project": id.0 })).unwrap();
        assert!(projects.backups.flush(Duration::from_secs(10)));
        assert_eq!(files_in(&settings).iter().filter(|n| n.ends_with(".suhina")).count(), 1);
        assert!(read_journal(&settings.join(JOURNAL)).is_empty());
        let other = projects.add(Session::new(blank_project()), None).unwrap();
        projects.execute("transport.setTempo", &json!({ "bpm": 90 })).unwrap();
        assert!(projects.finish_backups(Duration::from_secs(10)));
        assert_eq!(files_in(&settings).iter().filter(|n| n.ends_with(".suhina")).count(), 2, "quitting is critical");
        assert!(read_journal(&settings.join(JOURNAL)).is_empty());
        assert!(projects.get(other).is_some());
        let mut next_run = Projects::new();
        next_run.set_backup_folder(Some(settings.clone()));
        assert!(next_run.leftover_backups().is_empty());
        let _ = std::fs::remove_dir_all(settings);
    }

    #[test]
    fn autosave_checks_its_parameters_and_reports_without_a_project() {
        let mut projects = Projects::new();
        let info = projects.execute("project.autosave", &json!({ "interval": "4h", "clean_up": false })).unwrap();
        assert_eq!(
            (info["interval"].clone(), info["clean_up"].clone(), info["due"].clone()),
            (json!("4h"), json!(false), json!(false))
        );
        assert_eq!(info["last_backup"], Value::Null);
        assert!(projects.execute("project.autosave", &json!({ "interval": "2m" })).is_err());
        assert!(projects.execute("project.autosave", &json!({ "clean_up": "yes" })).is_err());
        assert!(projects.execute("project.autosave", &json!({ "now": true })).is_err(), "no project is open");
        // Untitled with no settings folder: nowhere to go, said so.
        projects.add(Session::new(blank_project()), None).unwrap();
        projects.execute("transport.setTempo", &json!({ "bpm": 100 })).unwrap();
        assert!(projects.execute("project.autosave", &json!({ "now": true })).is_err());
        // Unchanged: nothing needs writing.
        let mut clean = Projects::new();
        clean.add(Session::new(blank_project()), None).unwrap();
        assert_eq!(clean.execute("project.autosave", &json!({ "now": true })).unwrap()["written"], Value::Null);
    }

    #[test]
    fn intervals_have_names_and_titles_and_read_back() {
        let names: Vec<String> = Interval::ALL.iter().map(|i| i.name()).collect();
        assert_eq!(names, ["critical", "1m", "5m", "10m", "15m", "30m", "60m", "4h", "never"]);
        for interval in Interval::ALL {
            assert_eq!(Interval::parse(&interval.name()), Some(interval));
        }
        assert_eq!(Interval::Minutes(240).title(), "Every 4 hours");
        assert_eq!(Interval::Minutes(1).title(), "Every minute");
        assert_eq!(Interval::Minutes(60).title(), "Every 60 minutes");
        assert_eq!(Interval::Minutes(5).period(), Some(Duration::from_secs(300)));
        assert_eq!(Interval::Critical.period(), None);
        assert_eq!(Interval::parse("7m"), None);
        assert_eq!(Interval::default(), Interval::Minutes(5));
    }

    #[test]
    fn backup_names_are_recognised_and_others_are_not() {
        assert_eq!(project_name_of_file("Song 2026-10-15 14-05-09.suhina").as_deref(), Some("Song"));
        assert_eq!(project_name_of_file("My Song 2026-10-15 14-05-09 (12).suhina").as_deref(), Some("My Song"));
        assert_eq!(project_name_of_file("Song 2026-10-15 14:05:09.suhina"), None);
        assert_eq!(project_name_of_file("Song.suhina"), None);
        assert_eq!(project_name_of_file("Song 2026-10-15 14-05-09.wav"), None);
        assert_eq!(project_name_of_file(" 2026-10-15 14-05-09.suhina"), None);
        assert_eq!(project_name_of_file("journal.json"), None);
        assert_eq!(project_name_of_file("é 2026-10-15 14-05-0é.suhina"), None);
        assert_eq!(file_name_part("a/b: c?"), "a_b_ c_");
        assert_eq!(file_name_part("  ..  "), "Untitled");
    }

    #[test]
    fn a_broken_or_foreign_journal_is_empty() {
        assert!(parse_journal("not json").is_empty());
        assert!(parse_journal(r#"{"format":"other","backups":[]}"#).is_empty());
        let entries = parse_journal(
            r#"{"format":"suhina-backups","version":1,"backups":[
                {"run":"a","project":1,"file":null,"backup":"/x/Untitled 1 2026-10-15 14-05-09.suhina"},
                {"run":"a","project":-3,"backup":"/y"},
                {"run":"a","project":2,"file":"/m/Song.suhina","backup":"/m/.backup/Song 2026-10-15 14-05-09.suhina"}
            ]}"#,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(parse_journal(&journal_text(&entries)), entries);
    }

    fn temporary(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("suhina-backup-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clean_up_keeps_the_newest_of_one_project_and_touches_nothing_else() {
        let dir = temporary("clean");
        for second in 0..5 {
            std::fs::write(dir.join(format!("Song 2026-10-15 14-05-0{second}.suhina")), "x").unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }
        std::fs::write(dir.join("Song B 2026-10-15 14-05-09.suhina"), "x").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        clean_up(&dir, "Song", 2);
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(
            left,
            [
                "Song 2026-10-15 14-05-03.suhina",
                "Song 2026-10-15 14-05-04.suhina",
                "Song B 2026-10-15 14-05-09.suhina",
                "notes.txt"
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_entry_is_offered_only_while_its_backup_is_newer_than_its_file() {
        let dir = temporary("offer");
        let file = dir.join("Song.suhina");
        let backup = dir.join("Song 2026-10-15 14-05-09.suhina");
        std::fs::write(&file, "old").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&backup, "new").unwrap();
        let entry = Entry { run: "r".into(), project: 1, file: Some(file.clone()), backup: backup.clone() };
        assert!(offered(&entry).is_some());
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&file, "saved").unwrap();
        assert!(offered(&entry).is_none(), "saved after the backup");
        std::fs::remove_file(&file).unwrap();
        assert!(offered(&entry).is_some(), "the project file is gone");
        std::fs::remove_file(&backup).unwrap();
        assert!(offered(&entry).is_none(), "the backup is gone");
        let _ = std::fs::remove_dir_all(dir);
    }
}
