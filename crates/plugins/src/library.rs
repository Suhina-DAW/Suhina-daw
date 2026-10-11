//! The user's plug-in library: the folders they added, and the catalog of what the last scan
//! found. Both are kept as JSON in Loopwerk's settings folder, so a scan survives a restart and
//! the next one reads only what changed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::scan::{Catalog, Report, standard_folders};

/// How long reading one plug-in file may take before it counts as hung.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(10);
/// Folders a user may add.
const MAX_FOLDERS: usize = 256;
const FOLDERS_FILE: &str = "plugin-folders.json";
const CATALOG_FILE: &str = "plugin-catalog.json";

/// Loopwerk's settings folder: `%APPDATA%\Loopwerk` on Windows,
/// `~/Library/Application Support/Loopwerk` on macOS, `$XDG_CONFIG_HOME/loopwerk` (or
/// `~/.config/loopwerk`) elsewhere. `None` when the system says nothing about where.
pub fn settings_folder() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env("APPDATA").map(|p| p.join("Loopwerk"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library/Application Support/Loopwerk"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|p| p.join(".config"))).map(|p| p.join("loopwerk"))
    }
}

/// The `loopwerk-plugin-host` executable next to the running one.
pub fn host_next_to_this_program() -> PathBuf {
    let name = format!("loopwerk-plugin-host{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// `path` without Windows' `\\?\` prefix where it is an ordinary drive path, as people write it.
fn plain(path: PathBuf) -> PathBuf {
    let rest = {
        let text = path.to_string_lossy();
        text.strip_prefix(r"\\?\").filter(|rest| rest.as_bytes().get(1) == Some(&b':')).map(PathBuf::from)
    };
    rest.unwrap_or(path)
}

/// The folders the user added and the catalog, loaded from the settings folder on first use.
#[derive(Debug, Clone)]
pub struct Library {
    folder: Option<PathBuf>,
    host: PathBuf,
    loaded: bool,
    custom: Vec<PathBuf>,
    catalog: Catalog,
}

impl Default for Library {
    /// The library in Loopwerk's settings folder, scanned with the host next to this program.
    fn default() -> Self {
        Self::new(settings_folder(), host_next_to_this_program())
    }
}

impl Library {
    /// A library kept in `folder` (nothing is saved when it is `None`), scanned with `host`.
    pub fn new(folder: Option<PathBuf>, host: PathBuf) -> Self {
        Self { folder, host, loaded: false, custom: Vec::new(), catalog: Catalog::new() }
    }

    pub fn host(&self) -> &Path {
        &self.host
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(folder) = &self.folder else { return };
        if let Ok(text) = std::fs::read_to_string(folder.join(FOLDERS_FILE))
            && let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&text)
        {
            self.custom = list.iter().filter_map(Value::as_str).map(PathBuf::from).take(MAX_FOLDERS).collect();
        }
        if let Ok(text) = std::fs::read_to_string(folder.join(CATALOG_FILE)) {
            self.catalog = Catalog::from_json(&text);
        }
    }

    fn save(&self, name: &str, text: &str) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("this system has no settings folder to save in")?;
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = folder.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The folders the user added, in the order added.
    pub fn custom_folders(&mut self) -> &[PathBuf] {
        self.load();
        &self.custom
    }

    /// Every folder a scan looks in: the system's standard ones, then the user's.
    pub fn folders(&mut self) -> Vec<PathBuf> {
        let mut folders = standard_folders();
        for folder in self.custom_folders() {
            if !folders.contains(folder) {
                folders.push(folder.clone());
            }
        }
        folders
    }

    /// Adds a folder to look in; returns false if it was there already.
    pub fn add_folder(&mut self, folder: &Path) -> Result<bool, String> {
        self.load();
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        let folder = folder.canonicalize().map(plain).unwrap_or_else(|_| folder.to_path_buf());
        if self.custom.contains(&folder) {
            return Ok(false);
        }
        if self.custom.len() >= MAX_FOLDERS {
            return Err(format!("at most {MAX_FOLDERS} folders can be added"));
        }
        self.custom.push(folder);
        self.save_folders()?;
        Ok(true)
    }

    /// Stops looking in a folder the user added; returns false if it was not one of them.
    pub fn remove_folder(&mut self, folder: &Path) -> Result<bool, String> {
        self.load();
        let canonical = folder.canonicalize().ok().map(plain);
        let before = self.custom.len();
        self.custom.retain(|f| f != folder && Some(f) != canonical.as_ref());
        if self.custom.len() == before {
            return Ok(false);
        }
        self.save_folders()?;
        Ok(true)
    }

    fn save_folders(&self) -> Result<(), String> {
        let list: Vec<String> = self.custom.iter().map(|f| f.display().to_string()).collect();
        self.save(FOLDERS_FILE, &json!(list).to_string())
    }

    /// Scans every folder and saves the catalog. With `all`, forgets what earlier scans learned
    /// first, so every file is read again (failed ones too).
    pub fn scan(&mut self, all: bool) -> Result<Report, String> {
        self.load();
        if !self.host.exists() {
            return Err(format!("the plug-in host {} is missing; it ships next to Loopwerk", self.host.display()));
        }
        if all {
            self.catalog = Catalog::new();
        }
        let folders = self.folders();
        let report = self.catalog.scan(&self.host, &folders, SCAN_TIMEOUT);
        self.save(CATALOG_FILE, &self.catalog.to_json())?;
        Ok(report)
    }

    /// What the last scan found.
    pub fn catalog(&mut self) -> &Catalog {
        self.load();
        &self.catalog
    }
}
