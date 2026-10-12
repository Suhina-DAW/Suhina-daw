//! The themes the user can pick from: the built-in one and those read from theme files, and what
//! Settings › Interface › Theme does with them. The app reads and writes the files; this keeps
//! the list, says which themes changed and should be saved, and picks the colours to draw with.

use crate::settings_view::{ThemeRequest, ThemesView};
use crate::theme::{BUILT_IN, Palette, Theme};

/// A theme and its id: [`BUILT_IN`], or the name of its file without `.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    pub id: String,
    pub theme: Theme,
}

#[derive(Debug, Clone)]
pub struct Themes {
    /// The built-in theme first, then the files in the order the app gave them.
    entries: Vec<ThemeEntry>,
    folder: Option<String>,
    /// Ids of themes changed here, to be written to their files.
    unsaved: Vec<String>,
    /// The user asked to read the files again.
    reload: bool,
}

impl Default for Themes {
    fn default() -> Self {
        Self {
            entries: vec![ThemeEntry { id: BUILT_IN.into(), theme: Theme::default() }],
            folder: None,
            unsaved: Vec::new(),
            reload: false,
        }
    }
}

impl Themes {
    /// Uses the themes read from `folder`'s files. A file named like the built-in theme is left
    /// out, since the built-in one always comes first.
    pub fn set(&mut self, files: Vec<ThemeEntry>, folder: Option<String>) {
        self.entries.truncate(1);
        for entry in files {
            if !self.entries.iter().any(|e| same_id(&e.id, &entry.id)) {
                self.entries.push(entry);
            }
        }
        self.folder = folder;
    }

    /// One theme file read again, added if it is new. Returns whether anything changed.
    pub fn put(&mut self, entry: ThemeEntry) -> bool {
        if same_id(&entry.id, BUILT_IN) {
            return false;
        }
        match self.entries.iter_mut().find(|e| same_id(&e.id, &entry.id)) {
            Some(old) if *old == entry => false,
            Some(old) => {
                *old = entry;
                true
            }
            None => {
                self.entries.push(entry);
                true
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.entries.iter().find(|e| same_id(&e.id, id)).map(|e| &e.theme)
    }

    /// The colours of the theme `id`, or the built-in ones when there is no such theme.
    pub fn palette(&self, id: &str) -> Palette {
        self.get(id).map_or(Palette::DARK, |t| t.colors)
    }

    /// What Settings shows, with `current` in use.
    pub fn view(&self, current: &str) -> ThemesView {
        let shown = self.get(current).map_or(BUILT_IN, |_| current);
        let theme = self.get(shown).cloned().unwrap_or_default();
        ThemesView {
            list: self.entries.iter().map(|e| (e.id.clone(), e.theme.name.clone())).collect(),
            current: shown.to_string(),
            name: theme.name,
            colors: theme.colors,
            folder: self.folder.clone(),
        }
    }

    /// Does what the page asked while `current` is in use. Returns the id of the theme to use
    /// from now on, when that changes.
    pub fn request(&mut self, current: &str, request: ThemeRequest) -> Option<String> {
        match request {
            ThemeRequest::Use(id) => self.get(&id).is_some().then_some(id),
            ThemeRequest::Edit(colors) => {
                let (id, copied) = match self.get(current) {
                    Some(_) if !same_id(current, BUILT_IN) => (current.to_string(), false),
                    _ => (self.copy(current, "Custom"), true),
                };
                if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                    entry.theme.colors = colors;
                }
                self.mark(&id);
                copied.then_some(id)
            }
            ThemeRequest::Rename(name) => {
                if same_id(current, BUILT_IN) {
                    return None;
                }
                if let Some(entry) = self.entries.iter_mut().find(|e| same_id(&e.id, current)) {
                    entry.theme.name = name;
                    let id = entry.id.clone();
                    self.mark(&id);
                }
                None
            }
            ThemeRequest::Duplicate => {
                let name = format!("{} copy", self.get(current).map_or("Loopwerk Dark", |t| t.name.as_str()));
                let id = self.copy(current, &name);
                self.mark(&id);
                Some(id)
            }
            ThemeRequest::Reload => {
                self.reload = true;
                None
            }
        }
    }

    /// Themes changed since the app last asked, with their ids, for the app to write.
    pub fn take_unsaved(&mut self) -> Vec<(String, Theme)> {
        std::mem::take(&mut self.unsaved)
            .into_iter()
            .filter_map(|id| self.get(&id).map(|t| (id.clone(), t.clone())))
            .collect()
    }

    /// Whether the user asked to read the theme files again, once.
    pub fn take_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload)
    }

    /// Adds a copy of the theme `from` named `name`, under a new id. Returns the id.
    fn copy(&mut self, from: &str, name: &str) -> String {
        let colors = self.palette(from);
        let id = self.free_id(name);
        self.entries.push(ThemeEntry { id: id.clone(), theme: Theme { name: name.to_string(), colors } });
        id
    }

    /// An id made from `name` that no theme has yet: lower case, words joined by `-`.
    fn free_id(&self, name: &str) -> String {
        let mut base = String::new();
        for c in name.chars() {
            if c.is_alphanumeric() {
                base.extend(c.to_lowercase());
            } else if !base.is_empty() && !base.ends_with('-') {
                base.push('-');
            }
        }
        let base = base.trim_end_matches('-');
        let base = if base.is_empty() { "theme" } else { base };
        let mut id = base.to_string();
        let mut number = 2;
        while self.get(&id).is_some() {
            id = format!("{base}-{number}");
            number += 1;
        }
        id
    }

    fn mark(&mut self, id: &str) {
        if !self.unsaved.iter().any(|u| u == id) {
            self.unsaved.push(id.to_string());
        }
    }
}

/// Ids name files, and file names do not differ by case on every system.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn night() -> ThemeEntry {
        let mut colors = Palette::DARK;
        colors.accent = egui::Color32::from_rgb(1, 2, 3);
        ThemeEntry { id: "night".into(), theme: Theme { name: "Night".into(), colors } }
    }

    #[test]
    fn the_built_in_theme_comes_first_and_stays() {
        let mut themes = Themes::default();
        let fake = ThemeEntry { id: "Loopwerk-Dark".into(), theme: night().theme };
        themes.set(vec![night(), fake.clone(), night()], Some("themes".into()));
        let view = themes.view("night");
        assert_eq!(view.list, vec![(BUILT_IN.into(), "Loopwerk Dark".into()), ("night".into(), "Night".into())]);
        assert_eq!(view.colors, night().theme.colors);
        assert!(!themes.put(fake));
        assert_eq!(themes.palette(BUILT_IN), Palette::DARK);
    }

    #[test]
    fn a_missing_theme_draws_with_the_built_in_one() {
        let themes = Themes::default();
        assert_eq!(themes.palette("gone"), Palette::DARK);
        assert_eq!(themes.view("gone").current, BUILT_IN);
    }

    #[test]
    fn editing_the_built_in_theme_makes_a_copy() {
        let mut themes = Themes::default();
        let mut colors = Palette::DARK;
        colors.line = egui::Color32::from_rgb(9, 9, 9);
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Edit(colors)), Some("custom".into()));
        assert_eq!(themes.palette(BUILT_IN), Palette::DARK);
        assert_eq!(themes.palette("custom"), colors);
        // A second edit changes the copy, not another one.
        colors.line = egui::Color32::from_rgb(8, 8, 8);
        assert_eq!(themes.request("custom", ThemeRequest::Edit(colors)), None);
        assert_eq!(themes.take_unsaved(), vec![("custom".into(), Theme { name: "Custom".into(), colors })]);
        assert!(themes.take_unsaved().is_empty());
    }

    #[test]
    fn duplicates_get_their_own_ids() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy".into()));
        assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy-2".into()));
        assert_eq!(themes.get("night-copy-2").map(|t| t.colors), Some(night().theme.colors));
        assert_eq!(themes.take_unsaved().len(), 2);
    }

    #[test]
    fn only_theme_files_are_renamed() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Rename("Mine".into())), None);
        assert!(themes.take_unsaved().is_empty());
        themes.request("night", ThemeRequest::Rename("Midnight".into()));
        assert_eq!(themes.take_unsaved().first().map(|(_, t)| t.name.clone()), Some("Midnight".into()));
    }

    #[test]
    fn using_and_reloading() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("night".into())), Some("night".into()));
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("nothing".into())), None);
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Reload), None);
        assert!(themes.take_reload());
        assert!(!themes.take_reload());
    }

    #[test]
    fn ids_are_made_from_names() {
        let themes = Themes::default();
        assert_eq!(themes.free_id("  Night Blue!! "), "night-blue");
        assert_eq!(themes.free_id("../.."), "theme");
        assert_eq!(themes.free_id("Loopwerk Dark"), "loopwerk-dark-2");
    }
}
