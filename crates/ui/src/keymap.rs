//! Keyboard shortcuts: which keys run which action, kept in a file the user can edit
//! (`keymap.json`: an action id to a list of keys such as `"Ctrl+Shift+Z"`). An action is a
//! session command that needs no parameters, an action on the selection (which the window turns
//! into a command), or something the window itself does.

use std::collections::BTreeMap;

use egui::{Key, Modifiers};
use serde_json::{Value, json};

/// One key with its modifiers. `Ctrl` means Cmd on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Shortcut {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// Reads `Ctrl+Shift+Z`, `Space`, `Delete`, `Ctrl+,` and the like.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        // The key is after the last "+", unless the key is "+" itself.
        let (mods, key) = match text.rsplit_once('+') {
            Some((mods, "")) => (mods.strip_suffix('+').unwrap_or(mods), "+"),
            Some((mods, key)) => (mods, key),
            None => ("", text),
        };
        let mut modifiers = Modifiers::NONE;
        for part in mods.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => modifiers = modifiers.plus(Modifiers::COMMAND),
                "shift" => modifiers = modifiers.plus(Modifiers::SHIFT),
                "alt" | "option" => modifiers = modifiers.plus(Modifiers::ALT),
                _ => return None,
            }
        }
        let key = Key::from_name(key.trim()).or_else(|| Key::from_name(&capitalised(key.trim())))?;
        Some(Self { modifiers, key })
    }

    /// The shortcut as the keymap file and the menus write it.
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.command || self.modifiers.ctrl || self.modifiers.mac_cmd {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_string());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_string());
        }
        parts.push(self.key.name().to_string());
        parts.join("+")
    }

    fn modifier_count(&self) -> usize {
        usize::from(self.modifiers.command || self.modifiers.ctrl)
            + usize::from(self.modifiers.shift)
            + usize::from(self.modifiers.alt)
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| c.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase())
}

/// Something a key can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    /// Where it shows in the Shortcuts list.
    pub group: &'static str,
}

/// Actions the window does itself or on the selection, with their labels.
const WINDOW_ACTIONS: [(&str, &str, &str); 23] = [
    ("file.new", "New project or open one", "File"),
    ("file.open", "Open a project", "File"),
    ("file.save", "Save the project", "File"),
    ("file.saveAs", "Save the project under another name", "File"),
    ("file.saveTemplate", "Save the project as a template", "File"),
    ("transport.toggle", "Play or stop", "Transport"),
    ("view.toggle", "Switch between Session and Arrangement", "View"),
    ("view.session", "Show the Session view", "View"),
    ("view.arrangement", "Show the Arrangement view", "View"),
    ("settings.open", "Open or close Settings", "View"),
    ("view.detail", "Show or hide the detail area", "View"),
    ("view.browser", "Show or hide the browser", "View"),
    ("browser.search", "Search in the browser", "View"),
    ("view.sampleMap", "Show or hide the sample map in the main area", "View"),
    ("view.zoomIn", "Zoom the interface in", "View"),
    ("view.zoomOut", "Zoom the interface out", "View"),
    ("view.zoomReset", "Reset the interface zoom", "View"),
    ("selection.copy", "Copy the selected clip", "Edit"),
    ("selection.cut", "Cut the selected clip", "Edit"),
    ("selection.paste", "Paste after the selected clip, or at the playhead on the selected track", "Edit"),
    ("selection.duplicate", "Duplicate the selected clip", "Edit"),
    ("selection.delete", "Delete the selected clip", "Edit"),
    ("selection.group", "Group the selected tracks", "Tracks"),
];

/// Every action a key can be bound to: the window's, then each command that needs no
/// parameters.
pub fn actions() -> Vec<Action> {
    let mut all: Vec<Action> = WINDOW_ACTIONS
        .iter()
        .map(|(id, label, group)| Action { id: (*id).into(), label: (*label).into(), group })
        .collect();
    let session = loopwerk_session::commands().iter().map(|c| (c.id, c.label, c.params.iter().all(|p| !p.required)));
    let projects =
        loopwerk_session::project_commands().iter().map(|c| (c.id, c.label, c.params.iter().all(|p| !p.required)));
    for (id, label, no_params) in session.chain(projects) {
        // Saving goes through the window's own actions, which ask for a name the first time.
        if no_params && id != "project.save" && !all.iter().any(|a| a.id == id) {
            all.push(Action { id: id.into(), label: label.into(), group: "Commands" });
        }
    }
    all
}

/// The keys bound to each action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<String, Vec<Shortcut>>,
}

const DEFAULTS: [(&str, &[&str]); 24] = [
    ("file.new", &["Ctrl+T"]),
    ("file.open", &["Ctrl+O"]),
    ("file.save", &["Ctrl+S"]),
    ("file.saveAs", &["Ctrl+Shift+S"]),
    ("transport.toggle", &["Space"]),
    ("view.toggle", &["Tab"]),
    ("view.detail", &["Shift+Tab"]),
    ("view.browser", &["Ctrl+B"]),
    ("browser.search", &["Ctrl+F"]),
    ("view.sampleMap", &["Ctrl+Shift+M"]),
    ("view.zoomIn", &["Ctrl+Plus", "Ctrl+Equals"]),
    ("view.zoomOut", &["Ctrl+Minus"]),
    ("view.zoomReset", &["Ctrl+0"]),
    ("settings.open", &["Ctrl+Comma"]),
    ("edit.undo", &["Ctrl+Z"]),
    ("edit.redo", &["Ctrl+Shift+Z", "Ctrl+Y"]),
    ("selection.copy", &["Ctrl+C"]),
    ("selection.cut", &["Ctrl+X"]),
    ("selection.paste", &["Ctrl+V"]),
    ("selection.duplicate", &["Ctrl+D"]),
    ("selection.delete", &["Delete", "Backspace"]),
    ("selection.group", &["Ctrl+G"]),
    ("projects.next", &["Ctrl+Tab"]),
    ("projects.previous", &["Ctrl+Shift+Tab"]),
];

impl Default for Keymap {
    fn default() -> Self {
        let bindings = DEFAULTS
            .iter()
            .map(|(id, keys)| ((*id).to_string(), keys.iter().filter_map(|k| Shortcut::parse(k)).collect()))
            .collect();
        Self { bindings }
    }
}

impl Keymap {
    /// Reads a keymap file over the defaults: an action listed there takes the keys listed
    /// (an empty list unbinds it). Unknown actions and keys are skipped with a warning.
    pub fn from_json(text: &str) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut warnings = Vec::new();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return (keymap, vec!["the keymap file is not a JSON object; using the default keys".into()]);
        };
        let known = actions();
        for (id, keys) in map {
            if !known.iter().any(|a| a.id == id) {
                warnings.push(format!("keymap: {id} is not an action"));
                continue;
            }
            let texts: Vec<&str> = match &keys {
                Value::String(one) => vec![one.as_str()],
                Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
                _ => {
                    warnings.push(format!("keymap: {id} needs a key or a list of keys"));
                    continue;
                }
            };
            let mut parsed = Vec::new();
            for text in texts {
                match Shortcut::parse(text) {
                    Some(shortcut) => parsed.push(shortcut),
                    None => warnings.push(format!("keymap: {text:?} for {id} is not a key")),
                }
            }
            keymap.bindings.insert(id, parsed);
        }
        (keymap, warnings)
    }

    /// Every binding, as the keymap file keeps it.
    pub fn to_json(&self) -> String {
        let map: serde_json::Map<String, Value> = self
            .bindings
            .iter()
            .map(|(id, keys)| (id.clone(), json!(keys.iter().map(Shortcut::text).collect::<Vec<_>>())))
            .collect();
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    pub fn keys(&self, action: &str) -> &[Shortcut] {
        self.bindings.get(action).map_or(&[], Vec::as_slice)
    }

    /// The first key of an action as menus show it, if it has one.
    pub fn label(&self, action: &str) -> Option<String> {
        self.keys(action).first().map(Shortcut::text)
    }

    pub fn set(&mut self, action: &str, keys: Vec<Shortcut>) {
        self.bindings.insert(action.to_string(), keys);
    }

    /// Keys bound to more than one action, with those actions.
    pub fn conflicts(&self) -> Vec<(Shortcut, Vec<String>)> {
        let mut seen: Vec<(Shortcut, Vec<String>)> = Vec::new();
        for (id, keys) in &self.bindings {
            for key in keys {
                match seen.iter_mut().find(|(k, _)| k == key) {
                    Some((_, ids)) => ids.push(id.clone()),
                    None => seen.push((*key, vec![id.clone()])),
                }
            }
        }
        seen.retain(|(_, ids)| ids.len() > 1);
        seen
    }

    /// The actions whose keys were pressed this frame, taking the key presses so nothing else
    /// sees them. Longer combinations are tried first, so Ctrl+Shift+Z is not read as Ctrl+Z.
    pub fn pressed(&self, input: &mut egui::InputState) -> Vec<String> {
        let mut all: Vec<(&String, &Shortcut)> =
            self.bindings.iter().flat_map(|(id, keys)| keys.iter().map(move |k| (id, k))).collect();
        all.sort_by_key(|(_, k)| std::cmp::Reverse(k.modifier_count()));
        let mut fired = Vec::new();
        for (id, key) in all {
            let shortcut = egui::KeyboardShortcut::new(key.modifiers, key.key);
            let pressed = if key.modifiers.is_none() {
                input.consume_key(Modifiers::NONE, key.key)
            } else {
                input.consume_shortcut(&shortcut)
            };
            if pressed && !fired.contains(id) {
                fired.push(id.clone());
            }
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_and_write_the_same() {
        for text in [
            "Ctrl+Shift+Z",
            "Space",
            "Delete",
            "Ctrl+Comma",
            "Alt+F4",
            "Ctrl+Tab",
            "Ctrl+Plus",
            "Ctrl+Minus",
            "Ctrl+Equals",
            "Ctrl+0",
        ] {
            let shortcut = Shortcut::parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(Shortcut::parse(&shortcut.text()), Some(shortcut), "{text}");
        }
        assert_eq!(Shortcut::parse("ctrl+z"), Shortcut::parse("Ctrl+Z"));
        assert!(Shortcut::parse("Hyper+Z").is_none());
        assert!(Shortcut::parse("Ctrl+Nonsense").is_none());
    }

    #[test]
    fn a_keymap_file_overrides_the_defaults_and_warns_about_the_rest() {
        let (keymap, warnings) = Keymap::from_json(
            r#"{ "edit.undo": ["Alt+Z"], "selection.delete": [], "dance": ["X"], "edit.redo": ["Ctrl+Bogus"] }"#,
        );
        assert_eq!(keymap.label("edit.undo").as_deref(), Some("Alt+Z"));
        assert!(keymap.keys("selection.delete").is_empty());
        assert_eq!(keymap.label("transport.toggle").as_deref(), Some("Space"));
        assert_eq!(warnings.len(), 2);
        let (again, none) = Keymap::from_json(&keymap.to_json());
        assert_eq!((again, none.len()), (keymap, 0));
    }

    #[test]
    fn every_default_is_an_action_and_there_are_no_conflicts() {
        let known = actions();
        for (id, _) in DEFAULTS {
            assert!(known.iter().any(|a| a.id == id), "{id}");
        }
        assert!(Keymap::default().conflicts().is_empty());
        assert!(known.iter().any(|a| a.id == "transport.play"), "commands without parameters are actions");
        assert!(!known.iter().any(|a| a.id == "track.setVolume"), "commands that need parameters are not");
    }

    #[test]
    fn conflicts_are_found() {
        let mut keymap = Keymap::default();
        keymap.set("transport.play", vec![Shortcut::new(Modifiers::NONE, Key::Space)]);
        let conflicts = keymap.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1.len(), 2);
    }
}
