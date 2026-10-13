//! The library's background thread (ADR 0012): checks each folder against its saved index,
//! watches it for changes (or polls it when watching cannot be trusted), reads folders again on
//! request, and sends every new index back to the [`crate::Library`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};

use crate::index::FolderIndex;

/// How long events are gathered before the folders they name are read.
const DEBOUNCE: Duration = Duration::from_millis(500);
/// How often a polled folder is checked.
pub const POLL_EVERY: Duration = Duration::from_secs(300);

/// How a folder is kept up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    /// Being read for the first time, or checked at start-up.
    Scanning,
    /// Watched for changes by the system.
    Watching,
    /// Checked every few minutes.
    Polling,
    /// Not there (or not a folder).
    Missing,
    /// Not kept up to date: the library has not been started.
    Idle,
}

impl FolderState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Scanning => "scanning",
            Self::Watching => "watching",
            Self::Polling => "polling",
            Self::Missing => "missing",
            Self::Idle => "idle",
        }
    }
}

pub(crate) enum ToWorker {
    /// A folder to keep, with its saved index (if any) and where to save it.
    Add {
        root: PathBuf,
        saved: Option<FolderIndex>,
        file: Option<PathBuf>,
    },
    Remove(PathBuf),
    /// Read a folder (or every folder) again.
    Refresh(Option<PathBuf>),
    Events(Vec<PathBuf>, bool),
    WatchFailed,
    Stop,
}

pub(crate) enum FromWorker {
    Index(PathBuf, Arc<FolderIndex>),
    State(PathBuf, FolderState),
    /// One request of the library's is done.
    Idle,
}

struct Folder {
    index: FolderIndex,
    file: Option<PathBuf>,
    state: FolderState,
    last_poll: Instant,
    /// The folder's root with its symlinks resolved, when it has one: what FSEvents reports
    /// paths under (macOS), where the root as given (`/var` …) is a symlink (`/private/var` …).
    canonical: Option<PathBuf>,
}

/// Network file systems on Linux, whose changes the system does not report.
const NETWORK_FILE_SYSTEMS: [&str; 7] = ["nfs", "nfs4", "cifs", "smb3", "smbfs", "sshfs", "fuse.sshfs"];

/// Whether `path` is on a network drive, so watching it cannot be trusted.
pub fn on_network_drive(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        return text.starts_with(r"\\") && !text.starts_with(r"\\?\") && !text.starts_with(r"\\.\")
            || text.starts_with(r"\\?\UNC\");
    }
    if cfg!(target_os = "linux") {
        let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return false };
        // The longest mount point that holds the path decides.
        let mut best: Option<(usize, bool)> = None;
        for line in mounts.lines() {
            let mut parts = line.split_whitespace();
            let (_, Some(point), Some(kind)) = (parts.next(), parts.next(), parts.next()) else { continue };
            if path.starts_with(point) && best.is_none_or(|(len, _)| point.len() > len) {
                best = Some((point.len(), NETWORK_FILE_SYSTEMS.contains(&kind)));
            }
        }
        return best.is_some_and(|(_, network)| network);
    }
    false
}

/// On Linux, half the user's inotify watches; elsewhere no budget (one watch per folder).
fn watch_budget() -> Option<usize> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let limit = std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()
        .and_then(|t| t.trim().parse::<usize>().ok())
        .unwrap_or(8192);
    Some(limit / 2)
}

pub(crate) fn spawn(
    to_library: Sender<FromWorker>,
    inbox: Receiver<ToWorker>,
    own: Sender<ToWorker>,
) -> std::io::Result<()> {
    std::thread::Builder::new().name("loopwerk-library".into()).spawn(move || run(&to_library, &inbox, &own))?;
    Ok(())
}

fn run(out: &Sender<FromWorker>, inbox: &Receiver<ToWorker>, own: &Sender<ToWorker>) {
    let events = own.clone();
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| match result {
        Ok(list) => {
            let rescan = list.iter().any(|e| e.need_rescan());
            let paths: Vec<PathBuf> = list.into_iter().flat_map(|e| e.event.paths).collect();
            let _ = events.send(ToWorker::Events(paths, rescan));
        }
        Err(_) => {
            let _ = events.send(ToWorker::WatchFailed);
        }
    })
    .ok();
    let budget = watch_budget();
    let mut used_watches = 0usize;
    let mut folders: HashMap<PathBuf, Folder> = HashMap::new();
    let send_index = |root: &Path, folder: &Folder| {
        let _ = out.send(FromWorker::Index(root.to_path_buf(), Arc::new(folder.index.clone())));
        if let Some(file) = &folder.file {
            let _ = folder.index.save(root, file);
        }
    };
    loop {
        let message = match inbox.recv_timeout(Duration::from_secs(1)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // The library's own requests are answered with `Idle` once done, so it can wait for them.
        let requested = matches!(message, Some(ToWorker::Add { .. } | ToWorker::Remove(_) | ToWorker::Refresh(_)));
        match message {
            Some(ToWorker::Stop) => return,
            Some(ToWorker::Add { root, saved, file }) => {
                let _ = out.send(FromWorker::State(root.clone(), FolderState::Scanning));
                let mut folder = Folder {
                    index: saved.clone().unwrap_or_default(),
                    file,
                    state: FolderState::Scanning,
                    last_poll: Instant::now(),
                    canonical: std::fs::canonicalize(&root).ok(),
                };
                let changed = if saved.is_some() {
                    folder.index.refresh(&root)
                } else {
                    folder.index = FolderIndex::scan(&root);
                    true
                };
                let state = if crate::index::dir_time(&root).is_none() {
                    FolderState::Missing
                } else {
                    let dirs = folder.index.dirs.len();
                    let over_budget = budget.is_some_and(|b| used_watches + dirs > b);
                    let watched = !on_network_drive(&root)
                        && !over_budget
                        && debouncer.as_mut().is_some_and(|d| d.watch(&root, RecursiveMode::Recursive).is_ok());
                    if watched {
                        used_watches += dirs;
                        FolderState::Watching
                    } else {
                        FolderState::Polling
                    }
                };
                folder.state = state;
                if changed || saved.is_none() {
                    send_index(&root, &folder);
                }
                let _ = out.send(FromWorker::State(root.clone(), state));
                folders.insert(root, folder);
            }
            Some(ToWorker::Remove(root)) => {
                if let Some(folder) = folders.remove(&root)
                    && folder.state == FolderState::Watching
                {
                    used_watches = used_watches.saturating_sub(folder.index.dirs.len());
                    if let Some(d) = debouncer.as_mut() {
                        let _ = d.unwatch(&root);
                    }
                }
            }
            Some(ToWorker::Refresh(which)) => {
                for (root, folder) in folders.iter_mut().filter(|(r, _)| which.as_ref().is_none_or(|w| w == *r)) {
                    let _ = out.send(FromWorker::State(root.clone(), FolderState::Scanning));
                    // Asked for by the user: read whole, in case times did not move.
                    let index = FolderIndex::scan(root);
                    if index != folder.index {
                        folder.index = index;
                        send_index(root, folder);
                    }
                    // A folder that came back is polled; it is watched again from the next start.
                    let state = match (crate::index::dir_time(root).is_some(), folder.state) {
                        (false, _) => FolderState::Missing,
                        (true, FolderState::Missing) => FolderState::Polling,
                        (true, state) => state,
                    };
                    folder.state = state;
                    let _ = out.send(FromWorker::State(root.clone(), state));
                }
            }
            Some(ToWorker::Events(paths, rescan)) => {
                for (root, folder) in &mut folders {
                    // FSEvents (macOS) reports watched paths resolved through their symlinks
                    // (`/var` as `/private/var`), inotify the paths as they were given, so an
                    // event is this folder's when it is under the folder's canonical root too.
                    let base = folder.canonical.as_ref().filter(|c| paths.iter().any(|p| p.starts_with(c)));
                    let mine: Vec<PathBuf> = paths
                        .iter()
                        .filter(|p| p.starts_with(root) || base.is_some_and(|b| p.starts_with(b)))
                        .cloned()
                        .collect();
                    if mine.is_empty() && !rescan {
                        continue;
                    }
                    let changed = if rescan {
                        folder.index.refresh(root)
                    } else {
                        folder.index.update_paths(base.unwrap_or(root), &mine)
                    };
                    if changed {
                        send_index(root, folder);
                    }
                }
            }
            Some(ToWorker::WatchFailed) => {
                // Events may have been lost: every watched folder is checked and polled from now on.
                for (root, folder) in folders.iter_mut().filter(|(_, f)| f.state == FolderState::Watching) {
                    if folder.index.refresh(root) {
                        send_index(root, folder);
                    }
                    folder.state = FolderState::Polling;
                    let _ = out.send(FromWorker::State(root.clone(), FolderState::Polling));
                }
            }
            None => {
                let now = Instant::now();
                for (root, folder) in folders.iter_mut() {
                    if matches!(folder.state, FolderState::Polling | FolderState::Missing)
                        && now.duration_since(folder.last_poll) >= POLL_EVERY
                    {
                        folder.last_poll = now;
                        if folder.index.refresh(root) {
                            send_index(root, folder);
                        }
                        let present = crate::index::dir_time(root).is_some();
                        let state = if present { FolderState::Polling } else { FolderState::Missing };
                        if state != folder.state {
                            folder.state = state;
                            let _ = out.send(FromWorker::State(root.clone(), state));
                        }
                    }
                }
            }
        }
        if requested {
            let _ = out.send(FromWorker::Idle);
        }
    }
}
