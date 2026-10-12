//! Theme files: one JSON file per theme in the `themes` folder beside the settings file, named
//! by the theme's id (`night-blue.json`). A file saved while Loopwerk runs is read again within a
//! second, so a theme can be edited in any text editor and seen right away.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use loopwerk_ui::settings::is_theme_id;
use loopwerk_ui::theme::Theme;
use loopwerk_ui::themes::ThemeEntry;

/// Larger files are not themes; they are skipped instead of read.
const MAX_FILE: u64 = 256 * 1024;
/// How often the folder is looked at for changed files.
const CHECK_EVERY: Duration = Duration::from_secs(1);

pub struct ThemeFiles {
    folder: Option<PathBuf>,
    /// When each file was last changed, as last read or written.
    seen: HashMap<String, SystemTime>,
    checked: Option<Instant>,
}

impl ThemeFiles {
    pub fn new(folder: Option<PathBuf>) -> Self {
        Self { folder, seen: HashMap::new(), checked: None }
    }

    /// The folder as the Settings page shows it.
    pub fn folder_text(&self) -> Option<String> {
        self.folder.as_ref().map(|f| f.display().to_string())
    }

    /// Reads every theme file, in the order of their names, with warnings about the ones that
    /// could not be read whole.
    pub fn read_all(&mut self) -> (Vec<ThemeEntry>, Vec<String>) {
        self.seen.clear();
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        for (id, path, modified) in self.list() {
            if let Some(entry) = read(&id, &path, &mut warnings) {
                entries.push(entry);
            }
            self.seen.insert(id, modified);
        }
        (entries, warnings)
    }

    /// Files added or changed since they were last read, at most once every second.
    pub fn changed(&mut self) -> (Vec<ThemeEntry>, Vec<String>) {
        let now = Instant::now();
        if self.checked.is_some_and(|at| now.duration_since(at) < CHECK_EVERY) {
            return (Vec::new(), Vec::new());
        }
        self.checked = Some(now);
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        for (id, path, modified) in self.list() {
            if self.seen.get(&id) == Some(&modified) {
                continue;
            }
            if let Some(entry) = read(&id, &path, &mut warnings) {
                entries.push(entry);
            }
            self.seen.insert(id, modified);
        }
        (entries, warnings)
    }

    /// Writes the theme `id` to its file.
    pub fn write(&mut self, id: &str, theme: &Theme) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        if !is_theme_id(id) {
            return Err(format!("{id} cannot name a theme file"));
        }
        let path = folder.join(format!("{id}.json"));
        std::fs::create_dir_all(folder).map_err(|e| format!("could not save the theme {id}: {e}"))?;
        std::fs::write(&path, theme.to_json()).map_err(|e| format!("could not save the theme {id}: {e}"))?;
        if let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) {
            self.seen.insert(id.to_string(), modified);
        }
        Ok(())
    }

    /// Each `.json` file in the folder: its id, path and when it last changed, by name.
    fn list(&self) -> Vec<(String, PathBuf, SystemTime)> {
        let Some(Ok(dir)) = self.folder.as_ref().map(std::fs::read_dir) else { return Vec::new() };
        let mut files: Vec<_> = dir
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    return None;
                }
                let id = path.file_stem()?.to_str()?.to_string();
                let meta = entry.metadata().ok().filter(|m| m.is_file())?;
                Some((id, path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            })
            .filter(|(id, _, _)| is_theme_id(id))
            .collect();
        files.sort_by_key(|f| f.0.to_lowercase());
        files
    }
}

/// Reads one theme file; problems become warnings naming the file.
fn read(id: &str, path: &Path, warnings: &mut Vec<String>) -> Option<ThemeEntry> {
    let name = path.file_name().map_or_else(|| id.to_string(), |n| n.to_string_lossy().into_owned());
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_FILE => {
            warnings.push(format!("{name}: larger than a theme can be, skipped"));
            return None;
        }
        Ok(_) => {}
        Err(e) => {
            warnings.push(format!("{name}: {e}"));
            return None;
        }
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (theme, problems) = Theme::from_json(&text);
            warnings.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
            Some(ThemeEntry { id: id.to_string(), theme })
        }
        Err(e) => {
            warnings.push(format!("{name}: {e}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loopwerk-theme-files-{name}-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn themes_are_written_read_and_seen_when_they_change() {
        let dir = folder("round-trip");
        let mut files = ThemeFiles::new(Some(dir.clone()));
        assert_eq!(files.read_all(), (Vec::new(), Vec::new()));
        let theme = Theme { name: "Night".into(), ..Theme::default() };
        assert_eq!(files.write("night", &theme), Ok(()));
        assert!(files.write("../night", &theme).is_err());
        // Written here, so not read back as a change.
        assert!(files.changed().0.is_empty());

        std::fs::write(dir.join("b-bad.json"), "{ \"colors\": { \"accent\": \"red\" } }").unwrap_or_default();
        std::fs::write(dir.join("notes.txt"), "not a theme").unwrap_or_default();
        let (entries, warnings) = files.read_all();
        let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["b-bad", "night"]);
        assert_eq!(entries.get(1).map(|e| &e.theme), Some(&theme));
        assert_eq!(warnings.len(), 1, "{warnings:?}");

        files.checked = None;
        std::fs::write(dir.join("c-new.json"), "{ \"name\": \"New\" }").unwrap_or_default();
        let (changed, _) = files.changed();
        assert_eq!(changed.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["c-new"]);
        let _clean = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn huge_files_are_skipped() {
        let dir = folder("huge");
        std::fs::create_dir_all(&dir).unwrap_or_default();
        std::fs::write(dir.join("huge.json"), vec![b' '; MAX_FILE as usize + 1]).unwrap_or_default();
        let (entries, warnings) = ThemeFiles::new(Some(dir.clone())).read_all();
        assert!(entries.is_empty());
        assert_eq!(warnings.len(), 1);
        let _clean = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_folder_nothing_is_read_or_written() {
        let mut files = ThemeFiles::new(None);
        assert_eq!(files.read_all(), (Vec::new(), Vec::new()));
        assert!(files.write("night", &Theme::default()).is_err());
        assert_eq!(files.folder_text(), None);
    }
}
