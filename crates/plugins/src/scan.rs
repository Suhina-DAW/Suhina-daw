//! Finding installed plug-ins (ADR 0006, decision 9).
//!
//! [`standard_folders`] lists where each system keeps plug-ins; users can add folders of their
//! own. [`Catalog::scan`] looks through the folders for plug-in files and reads each new or
//! changed one in a `loopwerk-plugin-host --scan` process of its own, with a timeout, so a
//! plug-in that crashes or hangs while loading costs its own entry, nothing else. The catalog
//! keeps what it learned, keyed by path, modification time and size, so the next scan reads only
//! what changed; files that failed stay listed with the reason and are not tried again until
//! they change.
//!
//! CLAP and VST3.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde_json::{Value, json};

/// The line the host prints its scan result on; anything else a plug-in prints is ignored.
pub const SCAN_MARKER: &str = "LOOPWERK-SCAN ";
/// Folder levels a scan goes down.
const MAX_DEPTH: usize = 8;
/// Plug-in files one scan looks at, at most.
const MAX_FILES: usize = 10_000;
/// Bytes of a scanner's output read, at most.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
/// The saved catalog's format. A catalog saved in another one is dropped and everything is read
/// again (2: plug-ins list what they replace).
const CATALOG_VERSION: u64 = 2;

/// One plug-in a file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    /// The file (or bundle) that holds it.
    pub path: PathBuf,
    /// The plug-in's own stable id, such as `com.u-he.diva`.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// What it is, in the format's words (`audio-effect`, `instrument`, `stereo`, …).
    pub features: Vec<String>,
    /// Class ids (32 hex digits) of older plug-ins this one says it replaces and opens the
    /// saved state of: a VST3 plug-in's compatibility list (`moduleinfo.json` or
    /// `IPluginCompatibility`), which may name VST2 plug-ins by their derived class id
    /// ([`crate::matching::vst2_class_id`]).
    pub replaces: Vec<String>,
}

impl PluginInfo {
    /// Its format, from the file's extension: `vst3` or `clap`.
    pub fn format(&self) -> &'static str {
        if is_vst3(&self.path) { "vst3" } else { "clap" }
    }

    /// How the host is asked to load it (`clap:<file>#<id>` or `vst3:<file>#<id>`). A
    /// plug-in built into the host for tests has its name as its file (`test:vst3`) and is
    /// loaded by it.
    pub fn spec(&self) -> String {
        let path = self.path.display().to_string();
        if path.starts_with("test:") {
            return path;
        }
        format!("{}:{}#{}", self.format(), path, self.id)
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

fn is_vst3(path: &Path) -> bool {
    has_extension(path, "vst3")
}

/// What reading one file gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Scanned {
    Plugins(Vec<PluginInfo>),
    /// Why the file gave no plug-ins: it crashed, hung, or is not a plug-in.
    Failed(String),
}

/// One file in the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// Modification time (seconds since 1970) and size when it was read.
    pub modified: u64,
    pub size: u64,
    pub scanned: Scanned,
}

/// What a scan did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Files read for the first time or again because they changed.
    pub read: Vec<PathBuf>,
    /// Files whose plug-ins failed to load (read in this scan).
    pub failed: Vec<(PathBuf, String)>,
    /// Files no longer there, dropped from the catalog.
    pub removed: Vec<PathBuf>,
    /// Files that had not changed since the last scan.
    pub unchanged: usize,
}

/// Every plug-in file found so far, with what reading it gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    entries: Vec<Entry>,
}

/// Where this system keeps CLAP and VST3 plug-ins (those that exist), then the folders in
/// `CLAP_PATH`.
pub fn standard_folders() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut folders: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for format in ["CLAP", "VST3"] {
            folders.extend(env("COMMONPROGRAMFILES").map(|p| p.join(format)));
            folders.extend(env("LOCALAPPDATA").map(|p| p.join("Programs").join("Common").join(format)));
        }
    } else if cfg!(target_os = "macos") {
        for format in ["CLAP", "VST3"] {
            folders.extend(env("HOME").map(|p| p.join("Library/Audio/Plug-Ins").join(format)));
            folders.push(PathBuf::from("/Library/Audio/Plug-Ins").join(format));
        }
    } else {
        folders.extend(env("HOME").map(|p| p.join(".clap")));
        folders.push(PathBuf::from("/usr/lib/clap"));
        folders.push(PathBuf::from("/usr/local/lib/clap"));
        folders.extend(env("HOME").map(|p| p.join(".vst3")));
        folders.push(PathBuf::from("/usr/lib/vst3"));
        folders.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    if let Some(paths) = std::env::var_os("CLAP_PATH") {
        folders.extend(std::env::split_paths(&paths));
    }
    folders.retain(|f| f.is_dir());
    folders.dedup();
    folders
}

/// The plug-in files under `folders`: files or bundle folders named `*.clap` or `*.vst3` (a
/// bundle is not looked into), at most [`MAX_DEPTH`] levels down and [`MAX_FILES`] in all,
/// sorted.
pub fn find_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending: Vec<(PathBuf, usize)> = folders.iter().map(|f| (f.clone(), 0)).collect();
    while let Some((folder, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if has_extension(&path, "clap") || is_vst3(&path) {
                found.push(path);
                if found.len() >= MAX_FILES {
                    found.sort();
                    return found;
                }
            } else if depth < MAX_DEPTH && entry.file_type().is_ok_and(|t| t.is_dir()) {
                pending.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Modification time and size of `path` (for a bundle folder, of the folder itself).
fn stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    Some((modified, meta.len()))
}

/// Reads `file` in a `host --scan` process, waiting at most `timeout`. `file` may also be a
/// `test:` name the host understands, for tests.
pub fn scan_file(host: &Path, file: &str, timeout: Duration) -> Scanned {
    let child = crate::host_command(host)
        .args(["--scan", file])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Scanned::Failed(format!("could not start the scanner: {e}")),
    };
    let reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.take(MAX_OUTPUT).read_to_string(&mut text);
            text
        })
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = reader.and_then(|r| r.join().ok()).unwrap_or_default();
    let Some(status) = status else {
        return Scanned::Failed(format!("hung: no answer within {} s", timeout.as_secs_f32()));
    };
    let result = output
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(SCAN_MARKER))
        .and_then(|j| serde_json::from_str::<Value>(j).ok());
    match result {
        Some(value) => parse_scan(&value, file),
        None if status.success() => Scanned::Failed("the scanner gave no answer".into()),
        None => Scanned::Failed(format!("crashed while loading ({status})")),
    }
}

/// What the host prints for a scan that found `plugins` (their paths are not printed).
pub fn scan_json(plugins: &[PluginInfo]) -> String {
    let list: Vec<Value> = plugins.iter().map(plugin_json).collect();
    json!({ "plugins": list }).to_string()
}

/// What a scan worker prints for `file`: its plug-ins, or why it has none.
pub fn scan_answer(file: &str, result: Result<&[PluginInfo], &str>) -> String {
    let value = match result {
        Ok(plugins) => json!({ "file": file, "plugins": plugins.iter().map(plugin_json).collect::<Vec<_>>() }),
        Err(message) => json!({ "file": file, "error": message }),
    };
    value.to_string()
}

/// What the host prints for a scan that failed.
pub fn scan_error_json(message: &str) -> String {
    json!({ "error": message }).to_string()
}

fn plugin_json(p: &PluginInfo) -> Value {
    json!({
        "id": p.id, "name": p.name, "vendor": p.vendor, "version": p.version, "features": p.features,
        "replaces": p.replaces,
    })
}

/// Fills in [`PluginInfo::replaces`] from a VST3 compatibility list: an array of
/// `{ "New": <class id>, "Old": [<class id>, …] }`, the form both `moduleinfo.json` and
/// `IPluginCompatibility` use. Ids that are not 32 hex digits are skipped.
pub fn apply_compatibility(plugins: &mut [PluginInfo], list: &Value) {
    let class_id = |v: &Value| {
        v.as_str().map(str::to_ascii_uppercase).filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    for entry in list.as_array().into_iter().flatten() {
        let Some(new) = entry.get("New").and_then(class_id) else { continue };
        let old: Vec<String> =
            entry.get("Old").and_then(Value::as_array).into_iter().flatten().filter_map(class_id).collect();
        for plugin in plugins.iter_mut().filter(|p| p.id.eq_ignore_ascii_case(&new)) {
            for id in &old {
                if !plugin.replaces.contains(id) {
                    plugin.replaces.push(id.clone());
                }
            }
        }
    }
}

/// [`apply_compatibility`] from the list's text, as a plug-in writes it. It is read as JSON5,
/// which also takes the trailing commas some lists have; a trailing NUL is ignored and text
/// that cannot be read changes nothing.
pub fn apply_compatibility_text(plugins: &mut [PluginInfo], text: &str) {
    if let Ok(list) = json5::from_str::<Value>(text.trim_end_matches('\0')) {
        apply_compatibility(plugins, &list);
    }
}

/// The strings in the array `value[key]`.
fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|f| f.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

pub(crate) fn parse_scan(value: &Value, file: &str) -> Scanned {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Scanned::Failed(error.to_string());
    }
    let Some(list) = value.get("plugins").and_then(Value::as_array) else {
        return Scanned::Failed("the scanner's answer has no plug-ins".into());
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let plugins = list
        .iter()
        .filter(|p| !text(p, "id").is_empty())
        .map(|p| PluginInfo {
            path: PathBuf::from(file),
            id: text(p, "id"),
            name: text(p, "name"),
            vendor: text(p, "vendor"),
            version: text(p, "version"),
            features: strings(p, "features"),
            replaces: strings(p, "replaces"),
        })
        .collect::<Vec<_>>();
    if plugins.is_empty() { Scanned::Failed("the file holds no plug-ins".into()) } else { Scanned::Plugins(plugins) }
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Adds the plug-ins `path` holds as if a scan had read them (plug-ins described some other
    /// way, and tests with the host's own test plug-ins). Replaces what was known about `path`.
    pub fn insert(&mut self, path: PathBuf, plugins: Vec<PluginInfo>) {
        self.entries.retain(|e| e.path != path);
        self.entries.push(Entry { path, modified: 0, size: 0, scanned: Scanned::Plugins(plugins) });
    }

    /// Every plug-in found, in file order.
    pub fn plugins(&self) -> impl Iterator<Item = &PluginInfo> {
        self.entries
            .iter()
            .filter_map(|e| match &e.scanned {
                Scanned::Plugins(p) => Some(p.iter()),
                Scanned::Failed(_) => None,
            })
            .flatten()
    }

    /// The plug-in with this id, or else the first whose name matches (ignoring case).
    pub fn find(&self, id_or_name: &str) -> Option<&PluginInfo> {
        self.plugins()
            .find(|p| p.id == id_or_name)
            .or_else(|| self.plugins().find(|p| p.name.eq_ignore_ascii_case(id_or_name)))
    }

    /// Looks through `folders`, reads new and changed files, and drops files that are gone.
    /// VST3 bundles with a `moduleinfo.json` are read from it; the rest are loaded by
    /// [`scan_pool::default_workers`](crate::scan_pool::default_workers) worker processes, each
    /// file within `timeout` (shells within [`SHELL_TIMEOUT`](crate::scan_pool::SHELL_TIMEOUT)).
    pub fn scan(&mut self, host: &Path, folders: &[PathBuf], timeout: Duration) -> Report {
        use crate::scan_pool::{default_workers, module_info, scan_files, timeout_for};
        let files = find_files(folders);
        let mut report = Report::default();
        self.entries.retain(|e| {
            let keep = files.contains(&e.path);
            if !keep {
                report.removed.push(e.path.clone());
            }
            keep
        });
        let mut fresh: Vec<Entry> = Vec::new();
        let mut to_load: Vec<(PathBuf, u64, u64)> = Vec::new();
        for file in files {
            let Some((modified, size)) = stamp(&file) else { continue };
            if self.entries.iter().any(|e| e.path == file && e.modified == modified && e.size == size) {
                report.unchanged += 1;
                continue;
            }
            match module_info(&file) {
                Some(plugins) => fresh.push(Entry { path: file, modified, size, scanned: Scanned::Plugins(plugins) }),
                None => to_load.push((file, modified, size)),
            }
        }
        let names: Vec<String> = to_load.iter().map(|(f, _, _)| f.display().to_string()).collect();
        let loaded = scan_files(host, &names, default_workers(), &|f| timeout_for(f, timeout));
        for ((file, modified, size), mut scanned) in to_load.into_iter().zip(loaded) {
            if let Scanned::Plugins(plugins) = &mut scanned {
                plugins.iter_mut().for_each(|p| p.path = file.clone());
            }
            fresh.push(Entry { path: file, modified, size, scanned });
        }
        for entry in fresh {
            if let Scanned::Failed(why) = &entry.scanned {
                report.failed.push((entry.path.clone(), why.clone()));
            }
            report.read.push(entry.path.clone());
            match self.entries.iter_mut().find(|e| e.path == entry.path) {
                Some(slot) => *slot = entry,
                None => self.entries.push(entry),
            }
        }
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        report.read.sort();
        report
    }

    /// The catalog as JSON, for saving.
    pub fn to_json(&self) -> String {
        let entries: Vec<Value> = self
            .entries
            .iter()
            .map(|e| {
                let mut value = json!({ "path": e.path.display().to_string(), "modified": e.modified, "size": e.size });
                match &e.scanned {
                    Scanned::Plugins(plugins) => {
                        value["plugins"] = plugins.iter().map(plugin_json).collect();
                    }
                    Scanned::Failed(why) => value["failed"] = json!(why),
                }
                value
            })
            .collect();
        json!({ "version": CATALOG_VERSION, "entries": entries }).to_string()
    }

    /// A catalog saved by [`Catalog::to_json`]; anything unreadable gives an empty catalog, so
    /// the next scan reads everything again.
    pub fn from_json(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(text) else { return Self::default() };
        if value.get("version").and_then(Value::as_u64) != Some(CATALOG_VERSION) {
            return Self::default();
        }
        let entries = value
            .get("entries")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|e| {
                        let path = PathBuf::from(e.get("path")?.as_str()?);
                        let modified = e.get("modified")?.as_u64()?;
                        let size = e.get("size")?.as_u64()?;
                        let scanned = match e.get("failed").and_then(Value::as_str) {
                            Some(why) => Scanned::Failed(why.to_string()),
                            None => match parse_scan(e, &path.display().to_string()) {
                                Scanned::Plugins(p) => Scanned::Plugins(p),
                                Scanned::Failed(_) => return None,
                            },
                        };
                        Some(Entry { path, modified, size, scanned })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str) -> PluginInfo {
        PluginInfo {
            path: PathBuf::from("/vst3/Thing.vst3"),
            id: id.into(),
            name: "Thing".into(),
            vendor: "Maker".into(),
            version: "1".into(),
            features: Vec::new(),
            replaces: Vec::new(),
        }
    }

    #[test]
    fn a_compatibility_list_is_read_as_plugins_write_it() {
        let new = "ABCDEF019182FAEB49746D78426E6477";
        let mut plugins = [plugin(new), plugin("00000000000000000000000000000001")];
        // Lower-case ids, trailing commas and a trailing NUL, as some plug-ins write it; a
        // malformed id and a duplicate are left out.
        let text = format!(
            "[ {{ \"New\": \"{}\", \"Old\": [ \"5653544761696e6761696e0000000000\", \"short\", \
             \"5653544761696E6761696E0000000000\", ], }}, ]\0",
            new.to_lowercase()
        );
        apply_compatibility_text(&mut plugins, &text);
        assert_eq!(plugins[0].replaces, ["5653544761696E6761696E0000000000"]);
        assert!(plugins[1].replaces.is_empty());
        // Text that is not a list changes nothing.
        apply_compatibility_text(&mut plugins, "{ broken");
        assert_eq!(plugins[0].replaces.len(), 1);
    }
}
