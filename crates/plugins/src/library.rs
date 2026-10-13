//! The user's plug-in library: the folders they added, the catalog of what the last scan
//! found, and how each plug-in is hosted. All are kept as JSON in Loopwerk's settings folder,
//! so a scan survives a restart and the next one reads only what changed.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::scan::{Catalog, PluginInfo, Report, standard_folders};

/// How long loading one plug-in file may take before it counts as hung (shells get longer).
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
/// Folders a user may add.
const MAX_FOLDERS: usize = 256;
const FOLDERS_FILE: &str = "plugin-folders.json";
const CATALOG_FILE: &str = "plugin-catalog.json";
const HOSTING_FILE: &str = "plugin-hosting.json";
/// Plug-ins whose hosting may be set apart from the default.
const MAX_HOSTING: usize = 4096;
/// Longest hosting file read; a longer one is not Loopwerk's and is ignored.
const MAX_HOSTING_BYTES: u64 = 1 << 20;
/// Longest plug-in name (its spec) a hosting choice is kept for.
const MAX_SPEC: usize = 4096;

/// Which host process a plug-in's instances run in (ADR 0006, decision 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hosting {
    /// One process per plug-in file: every instance of every plug-in in the file shares it.
    #[default]
    Binary,
    /// A process of its own for every instance.
    Instance,
    /// One process per vendor: every instance of the vendor's plug-ins shares it.
    Vendor,
}

impl Hosting {
    /// The name it is saved and shown as: `binary`, `instance` or `vendor`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Instance => "instance",
            Self::Vendor => "vendor",
        }
    }

    /// The hosting [`Hosting::name`] gives `text`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "binary" => Some(Self::Binary),
            "instance" => Some(Self::Instance),
            "vendor" => Some(Self::Vendor),
            _ => None,
        }
    }
}

/// The process group ([`crate::Launch::group`]) an instance of `plugin` starts in under
/// `hosting`: its file's (`file:<path>`, shared by every plug-in in the file or shell), its
/// vendor's (`vendor:<name>`, or the file's when the vendor is unknown), or none (a process of
/// its own).
pub fn group_for(hosting: Hosting, plugin: &PluginInfo) -> Option<String> {
    let file = || format!("file:{}", plugin.path.display());
    match hosting {
        Hosting::Binary => Some(file()),
        Hosting::Vendor if plugin.vendor.trim().is_empty() => Some(file()),
        Hosting::Vendor => Some(format!("vendor:{}", plugin.vendor.trim())),
        Hosting::Instance => None,
    }
}

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
    /// Whether scans look in the system's standard folders as well as the added ones.
    standard: bool,
    /// Plug-ins (by spec) hosted other than the default way.
    hosting: BTreeMap<String, Hosting>,
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
        Self {
            folder,
            host,
            loaded: false,
            custom: Vec::new(),
            catalog: Catalog::new(),
            standard: true,
            hosting: BTreeMap::new(),
        }
    }

    /// Scans only the folders the user added, not the system's standard ones (tests, portable
    /// setups).
    pub fn without_standard_folders(mut self) -> Self {
        self.standard = false;
        self
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
        if let Some(text) = read_bounded(&folder.join(HOSTING_FILE), MAX_HOSTING_BYTES) {
            self.hosting = parse_hosting(&text);
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
        let mut folders = if self.standard { standard_folders() } else { Vec::new() };
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

    /// How the plug-in `plugin_spec` ([`PluginInfo::spec`]) is hosted; [`Hosting::Binary`]
    /// unless the user chose otherwise.
    pub fn hosting(&mut self, plugin_spec: &str) -> Hosting {
        self.load();
        self.hosting.get(plugin_spec).copied().unwrap_or_default()
    }

    /// Chooses how the plug-in `plugin_spec` is hosted from its next start, and saves it.
    pub fn set_hosting(&mut self, plugin_spec: &str, hosting: Hosting) -> Result<(), String> {
        self.load();
        if plugin_spec.is_empty() || plugin_spec.len() > MAX_SPEC {
            return Err("not a plug-in name".into());
        }
        if hosting == Hosting::default() {
            self.hosting.remove(plugin_spec);
        } else {
            if !self.hosting.contains_key(plugin_spec) && self.hosting.len() >= MAX_HOSTING {
                return Err(format!("at most {MAX_HOSTING} plug-ins can be hosted other than the default way"));
            }
            self.hosting.insert(plugin_spec.to_string(), hosting);
        }
        let list: Map<String, Value> =
            self.hosting.iter().map(|(spec, hosting)| (spec.clone(), Value::from(hosting.name()))).collect();
        self.save(HOSTING_FILE, &Value::Object(list).to_string())
    }
}

/// The text of the file at `path`, when it is there and at most `limit` bytes long.
fn read_bounded(path: &Path, limit: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= limit).then_some(text)
}

/// The hosting choices in a `plugin-hosting.json` text: an object from plug-in spec to hosting
/// name. Entries that are not that are skipped, and so are the default ones.
fn parse_hosting(text: &str) -> BTreeMap<String, Hosting> {
    let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(text) else {
        return BTreeMap::new();
    };
    entries
        .iter()
        .filter(|(spec, _)| !spec.is_empty() && spec.len() <= MAX_SPEC)
        .filter_map(|(spec, hosting)| Some((spec.clone(), Hosting::parse(hosting.as_str()?)?)))
        .filter(|(_, hosting)| *hosting != Hosting::default())
        .take(MAX_HOSTING)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("loopwerk-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn info(path: &str, vendor: &str) -> PluginInfo {
        PluginInfo {
            path: PathBuf::from(path),
            id: "com.example.thing".into(),
            name: "Thing".into(),
            vendor: vendor.into(),
            version: String::new(),
            features: Vec::new(),
            replaces: Vec::new(),
        }
    }

    #[test]
    fn hosting_names_round_trip() {
        for hosting in [Hosting::Binary, Hosting::Instance, Hosting::Vendor] {
            assert_eq!(Hosting::parse(hosting.name()), Some(hosting));
        }
        assert_eq!(Hosting::parse("Binary"), None);
        assert_eq!(Hosting::parse(""), None);
        assert_eq!(Hosting::default(), Hosting::Binary);
    }

    #[test]
    fn hosting_is_saved_and_read_back() {
        let folder = scratch("hosting");
        let mut library = Library::new(Some(folder.clone()), PathBuf::from("host"));
        assert_eq!(library.hosting("clap:a.clap#x"), Hosting::Binary);
        library.set_hosting("clap:a.clap#x", Hosting::Instance).unwrap();
        library.set_hosting("vst3:b.vst3#00", Hosting::Vendor).unwrap();
        library.set_hosting("vst3:c.vst3#11", Hosting::Vendor).unwrap();
        library.set_hosting("vst3:c.vst3#11", Hosting::Binary).unwrap();
        assert!(library.set_hosting("", Hosting::Vendor).is_err());

        let mut again = Library::new(Some(folder.clone()), PathBuf::from("host"));
        assert_eq!(again.hosting("clap:a.clap#x"), Hosting::Instance);
        assert_eq!(again.hosting("vst3:b.vst3#00"), Hosting::Vendor);
        assert_eq!(again.hosting("vst3:c.vst3#11"), Hosting::Binary);
        assert_eq!(again.hosting("unknown"), Hosting::Binary);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_malformed_hosting_file_is_ignored() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"a":5,"b":"nonsense","c":null,"":"vendor","d":"vendor"}"#,
            &format!("{{\"e\":\"{}\"}}", "x".repeat(2 << 20)),
        ] {
            let folder = scratch("malformed");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(HOSTING_FILE), text).unwrap();
            let mut library = Library::new(Some(folder.clone()), PathBuf::from("host"));
            for spec in ["a", "b", "c", "", "e"] {
                assert_eq!(library.hosting(spec), Hosting::Binary, "{spec} in {:.40}", text);
            }
            let expected = if text.contains("\"d\"") { Hosting::Vendor } else { Hosting::Binary };
            assert_eq!(library.hosting("d"), expected);
            let _ = std::fs::remove_dir_all(&folder);
        }
        let many: Map<String, Value> =
            (0..MAX_HOSTING + 10).map(|n| (format!("p{n}"), Value::from("vendor"))).collect();
        assert_eq!(parse_hosting(&Value::Object(many).to_string()).len(), MAX_HOSTING);
    }

    #[test]
    fn groups_follow_the_hosting() {
        let plugin = info("C:/Plugins/Thing.vst3", "Example Audio");
        assert_eq!(group_for(Hosting::Binary, &plugin).as_deref(), Some("file:C:/Plugins/Thing.vst3"));
        assert_eq!(group_for(Hosting::Vendor, &plugin).as_deref(), Some("vendor:Example Audio"));
        assert_eq!(group_for(Hosting::Instance, &plugin), None);
        let unknown = info("C:/Plugins/Other.clap", " ");
        assert_eq!(group_for(Hosting::Vendor, &unknown).as_deref(), Some("file:C:/Plugins/Other.clap"));
        // Two plug-ins in one file share its process.
        let sibling = PluginInfo { id: "com.example.other".into(), ..plugin.clone() };
        assert_eq!(group_for(Hosting::Binary, &sibling), group_for(Hosting::Binary, &plugin));
    }
}
