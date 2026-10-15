//! Keyboard shortcuts: which keys run which action, kept in a file the user can edit
//! (`keymap.json`: an action id to a list of keys such as `"Ctrl+Shift+Z"`, and the name of the
//! `preset` the keys came from). An action is a
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
const WINDOW_ACTIONS: [(&str, &str, &str); 52] = [
    ("file.new", "New project or open one", "File"),
    ("file.open", "Open a project", "File"),
    ("file.save", "Save the project", "File"),
    ("file.saveAs", "Save the project under another name", "File"),
    ("file.saveTemplate", "Save the project as a template", "File"),
    ("transport.toggle", "Play or stop", "Transport"),
    ("transport.playFromSelection", "Play from the start of the selected clip", "Transport"),
    ("view.toggle", "Switch between Session and Arrangement", "View"),
    ("view.session", "Show the Session view", "View"),
    ("view.arrangement", "Show the Arrangement view", "View"),
    ("settings.open", "Open or close Settings", "View"),
    ("view.detail", "Show or hide the detail area", "View"),
    ("view.mappingMode", "Map keys to clip slots, scenes and stop buttons, or stop mapping", "View"),
    ("view.browser", "Show or hide the browser", "View"),
    ("browser.search", "Search in the browser", "View"),
    ("view.sampleMap", "Show or hide the sample map in the main area", "View"),
    ("view.sampleMapWindow", "Open the sample map in a window of its own, or put it back", "View"),
    ("view.zoomIn", "Make the whole window larger", "View"),
    ("view.zoomOut", "Make the whole window smaller", "View"),
    ("view.zoomReset", "Make the whole window its normal size", "View"),
    ("selection.copy", "Copy the selected clips, or the selected notes in the piano roll", "Edit"),
    ("selection.cut", "Cut the selected clips, or the selected notes in the piano roll", "Edit"),
    (
        "selection.paste",
        "Paste after the selected clip, or at the playhead on the selected track; notes in the piano roll",
        "Edit",
    ),
    ("selection.duplicate", "Duplicate the selected clips, or the selected notes in the piano roll", "Edit"),
    ("selection.delete", "Delete the selected clips, or the selected notes in the piano roll", "Edit"),
    ("selection.mute", "Mute or unmute the selected clips", "Edit"),
    ("selection.selectAll", "Select every clip in the arrangement", "Edit"),
    ("selection.group", "Group the selected tracks", "Tracks"),
    ("selection.launch", "Launch the selected clip slot", "Session"),
    ("selection.up", "Select the clip slot above", "Session"),
    ("selection.down", "Select the clip slot below", "Session"),
    ("selection.left", "Select the clip slot in the track to the left", "Session"),
    ("selection.right", "Select the clip slot in the track to the right", "Session"),
    ("pianoRoll.quantize", "Quantise the piano roll's selected notes, or all its notes", PIANO_ROLL),
    ("pianoRoll.selectAll", "Select every note in the piano roll", PIANO_ROLL),
    ("pianoRoll.deselect", "Clear the piano roll's note selection", PIANO_ROLL),
    ("pianoRoll.earlier", "Move the selected notes one grid step earlier", PIANO_ROLL),
    ("pianoRoll.later", "Move the selected notes one grid step later", PIANO_ROLL),
    ("pianoRoll.up", "Move the selected notes a semitone up", PIANO_ROLL),
    ("pianoRoll.down", "Move the selected notes a semitone down", PIANO_ROLL),
    ("pianoRoll.octaveUp", "Move the selected notes an octave up", PIANO_ROLL),
    ("pianoRoll.octaveDown", "Move the selected notes an octave down", PIANO_ROLL),
    ("pianoRoll.shorter", "Make the selected notes one grid step shorter", PIANO_ROLL),
    ("pianoRoll.longer", "Make the selected notes one grid step longer", PIANO_ROLL),
    ("pianoRoll.draw", "Switch the piano roll between the Select and Draw tools", PIANO_ROLL),
    ("pianoRoll.tool1", "Choose the Select tool", PIANO_ROLL),
    ("pianoRoll.tool2", "Choose the Draw tool", PIANO_ROLL),
    ("pianoRoll.tool3", "Choose the Paint tool", PIANO_ROLL),
    ("pianoRoll.tool4", "Choose the Erase tool", PIANO_ROLL),
    ("pianoRoll.tool5", "Choose the Slice tool", PIANO_ROLL),
    ("marker.addAtPlayhead", "Add an arrangement marker at the playhead", "Edit"),
    ("help.about", "Show which version of Suhina this is", "Help"),
];

/// The Shortcuts list's group for the piano roll's own actions.
const PIANO_ROLL: &str = "Piano roll";

/// Whether an action works only while the piano roll has the keyboard (after a click in it). Its
/// keys are left for the rest of the window otherwise: Escape and the arrows mean other things
/// elsewhere. Quantise (Ctrl+U) means nothing elsewhere, so it works whenever the piano roll
/// shows a clip.
pub fn piano_roll_only(action: &str) -> bool {
    action.starts_with("pianoRoll.") && action != "pianoRoll.quantize"
}

/// Whether `action` works only on a selected clip slot in the Session view: the piano roll keys
/// never do (it edits arrangement clips), so the two may share keys.
pub fn slot_only(action: &str) -> bool {
    matches!(action, "selection.launch" | "selection.up" | "selection.down" | "selection.left" | "selection.right")
}

/// Whether `action` is left alone while the piano roll has the keyboard: the selected slot's keys,
/// and selecting every arrangement clip, whose key selects every note there instead.
pub fn not_in_piano_roll(action: &str) -> bool {
    slot_only(action) || action == "selection.selectAll"
}

/// Whether `a` and `b` can both run on the same key press, so one key for both is a conflict.
fn overlap(a: &str, b: &str) -> bool {
    !((piano_roll_only(a) && not_in_piano_roll(b)) || (not_in_piano_roll(a) && piano_roll_only(b)))
}

/// Every action a key can be bound to: the window's, then each command that needs no
/// parameters.
pub fn actions() -> Vec<Action> {
    let mut all: Vec<Action> = WINDOW_ACTIONS
        .iter()
        .map(|(id, label, group)| Action { id: (*id).into(), label: (*label).into(), group })
        .collect();
    let session = suhina_session::commands().iter().map(|c| (c.id, c.label, c.params.iter().all(|p| !p.required)));
    let projects =
        suhina_session::project_commands().iter().map(|c| (c.id, c.label, c.params.iter().all(|p| !p.required)));
    for (id, label, no_params) in session.chain(projects) {
        // Saving goes through the window's own actions, which ask for a name the first time.
        if no_params && id != "project.save" && !all.iter().any(|a| a.id == id) {
            all.push(Action { id: id.into(), label: label.into(), group: "Commands" });
        }
    }
    all
}

/// The keys bound to each action, and the preset the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<String, Vec<Shortcut>>,
    preset: Preset,
}

/// A set of keys for a way of working, chosen in Settings › Shortcuts (› Keymap presets in
/// `docs/spec/ui-clip-editor.md`). The names describe the habit, not another program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// Suhina's own keys: duplicate is Ctrl+D, a right-click on a note does nothing.
    #[default]
    SelectFirst,
    /// Drawing notes first: duplicate is Ctrl+B (which the browser's Ctrl+B gives up) and a
    /// right-click on a note deletes it in the Draw and Paint tools.
    DrawFirst,
}

impl Preset {
    pub const ALL: [Self; 2] = [Self::SelectFirst, Self::DrawFirst];

    /// The name the menu and the keymap file use.
    pub fn name(self) -> &'static str {
        match self {
            Self::SelectFirst => "Select-first",
            Self::DrawFirst => "Draw-first",
        }
    }

    /// The preset `name` names; `None` when it is not one of them (an edited file, a rename).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }

    /// What the preset says beyond the default keys, in its own words: the Settings row shows it.
    pub fn note(self) -> &'static str {
        match self {
            Self::SelectFirst => "Duplicate is Ctrl+D; a right-click on a note does nothing",
            Self::DrawFirst => "Duplicate is Ctrl+B; in Draw and Paint a right-click on a note deletes it",
        }
    }

    /// Whether a right-click on a note deletes it in the Draw and Paint tools.
    pub fn right_click_deletes(self) -> bool {
        self == Self::DrawFirst
    }

    /// The key the preset gives the duplicate action, as the Shortcuts list shows it.
    pub fn duplicate_key(self) -> &'static str {
        match self {
            Self::SelectFirst => "Ctrl+D",
            Self::DrawFirst => "Ctrl+B",
        }
    }
}

/// The keymap file's entry for the preset.
const PRESET_KEY: &str = "preset";

const DEFAULTS: [(&str, &[&str]); 53] = [
    ("file.new", &["Ctrl+T"]),
    ("file.open", &["Ctrl+O"]),
    ("file.save", &["Ctrl+S"]),
    ("file.saveAs", &["Ctrl+Shift+S"]),
    ("transport.toggle", &["Space"]),
    ("transport.playFromSelection", &["Shift+Space"]),
    ("view.toggle", &["Tab"]),
    ("view.detail", &["Shift+Tab"]),
    ("view.mappingMode", &["Ctrl+M"]),
    ("view.browser", &["Ctrl+B"]),
    ("browser.search", &["Ctrl+F"]),
    ("view.sampleMap", &["Ctrl+Shift+M"]),
    ("view.sampleMapWindow", &["Ctrl+Alt+M"]),
    ("settings.open", &["Ctrl+Comma"]),
    ("view.zoomIn", &["Ctrl+Plus", "Ctrl+Equals"]),
    ("view.zoomOut", &["Ctrl+Minus"]),
    ("view.zoomReset", &["Ctrl+0"]),
    ("edit.undo", &["Ctrl+Z"]),
    ("edit.redo", &["Ctrl+Shift+Z", "Ctrl+Y"]),
    ("selection.copy", &["Ctrl+C"]),
    ("selection.cut", &["Ctrl+X"]),
    ("selection.paste", &["Ctrl+V"]),
    ("selection.duplicate", &["Ctrl+D"]),
    ("selection.delete", &["Delete", "Backspace"]),
    ("selection.mute", &["0"]),
    ("selection.selectAll", &["Ctrl+A"]),
    ("selection.group", &["Ctrl+G"]),
    ("selection.launch", &["Enter"]),
    ("selection.up", &["Up"]),
    ("selection.down", &["Down"]),
    ("selection.left", &["Left"]),
    ("selection.right", &["Right"]),
    ("pianoRoll.quantize", &["Ctrl+U"]),
    ("marker.addAtPlayhead", &["M"]),
    ("transport.setLoopEnabled", &["Ctrl+L"]),
    ("projects.next", &["Ctrl+Tab"]),
    ("projects.previous", &["Ctrl+Shift+Tab"]),
    ("pianoRoll.selectAll", &["Ctrl+A"]),
    ("pianoRoll.deselect", &["Escape"]),
    ("pianoRoll.earlier", &["Left"]),
    ("pianoRoll.later", &["Right"]),
    ("pianoRoll.up", &["Up"]),
    ("pianoRoll.down", &["Down"]),
    ("pianoRoll.octaveUp", &["Shift+Up"]),
    ("pianoRoll.octaveDown", &["Shift+Down"]),
    ("pianoRoll.shorter", &["Shift+Left"]),
    ("pianoRoll.longer", &["Shift+Right"]),
    ("pianoRoll.draw", &["B"]),
    ("pianoRoll.tool1", &["1"]),
    ("pianoRoll.tool2", &["2"]),
    ("pianoRoll.tool3", &["3"]),
    ("pianoRoll.tool4", &["4"]),
    ("pianoRoll.tool5", &["5"]),
];

impl Default for Keymap {
    fn default() -> Self {
        Self { bindings: bindings_for(&DEFAULTS), preset: Preset::default() }
    }
}

/// The keys of `defaults`, read from their text.
fn bindings_for(defaults: &[(&str, &[&str])]) -> BTreeMap<String, Vec<Shortcut>> {
    defaults
        .iter()
        .map(|(id, keys)| ((*id).to_string(), keys.iter().filter_map(|k| Shortcut::parse(k)).collect()))
        .collect()
}

impl Keymap {
    /// Reads a keymap file over the defaults: an action listed there takes the keys listed
    /// (an empty list unbinds it), `preset` names the set the keys came from. Unknown actions,
    /// keys and presets are skipped with a warning; an unknown preset leaves the default one.
    pub fn from_json(text: &str) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut warnings = Vec::new();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return (keymap, vec!["the keymap file is not a JSON object; using the default keys".into()]);
        };
        if let Some(named) = map.get(PRESET_KEY) {
            match named.as_str().and_then(Preset::from_name) {
                Some(preset) => keymap.preset = preset,
                None => warnings.push(format!(
                    "keymap: {} is not a preset the program knows; using {}",
                    named,
                    Preset::default().name()
                )),
            }
        }
        let known = actions();
        for (id, keys) in map {
            if id == PRESET_KEY {
                continue;
            }
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

    /// Every binding and the preset, as the keymap file keeps them.
    pub fn to_json(&self) -> String {
        let mut map: serde_json::Map<String, Value> = self
            .bindings
            .iter()
            .map(|(id, keys)| (id.clone(), json!(keys.iter().map(Shortcut::text).collect::<Vec<_>>())))
            .collect();
        map.insert(PRESET_KEY.to_string(), json!(self.preset.name()));
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    /// The preset the keys came from.
    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// Puts the keys of `preset` back: every default, with the preset's own changes. Keys changed
    /// by hand go back to the preset's; everything else keeps what it has.
    pub fn set_preset(&mut self, preset: Preset) {
        let defaults = Self::default();
        let mut bindings = defaults.bindings;
        if preset == Preset::DrawFirst {
            // Duplicate takes Ctrl+B; the browser keeps a key of its own, Ctrl+Shift+B.
            let ctrl_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
            bindings.insert("view.browser".to_string(), vec![Shortcut::new(ctrl_shift, Key::B)]);
            bindings.insert("selection.duplicate".to_string(), vec![Shortcut::new(Modifiers::COMMAND, Key::B)]);
        }
        self.bindings = bindings;
        self.preset = preset;
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
        seen.retain(|(_, ids)| ids.iter().enumerate().any(|(i, a)| ids[i + 1..].iter().any(|b| overlap(a, b))));
        seen
    }

    /// The actions whose keys were pressed this frame, of those `wanted` lets through, taking
    /// their key presses so nothing else sees them; the keys of the other actions are left alone.
    /// Longer combinations are tried first, so Ctrl+Shift+Z is not read as Ctrl+Z.
    pub fn pressed(&self, input: &mut egui::InputState, wanted: impl Fn(&str) -> bool) -> Vec<String> {
        clipboard_keys(input);
        let mut all: Vec<(&String, &Shortcut)> = self
            .bindings
            .iter()
            .filter(|(id, _)| wanted(id))
            .flat_map(|(id, keys)| keys.iter().map(move |k| (id, k)))
            .collect();
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

/// The window reports Ctrl+X, Ctrl+C and Ctrl+V (Cmd on macOS) as cut, copy and paste events
/// rather than as keys (and Ctrl+V only when the system clipboard holds text). They become their
/// keys again here, so the keymap's actions for them fire (`selection.cut`, `.copy`, `.paste`).
fn clipboard_keys(input: &mut egui::InputState) {
    let modifiers = input.modifiers;
    for event in &mut input.events {
        let key = match event {
            egui::Event::Cut => Key::X,
            egui::Event::Copy => Key::C,
            egui::Event::Paste(_) => Key::V,
            _ => continue,
        };
        *event = egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: modifiers.plus(Modifiers::COMMAND),
        };
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
            "Minus",
            "Enter",
            "Up",
            "Escape",
            "Shift+Up",
            "Left",
            "Ctrl+A",
        ] {
            let shortcut = Shortcut::parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(Shortcut::parse(&shortcut.text()), Some(shortcut), "{text}");
        }
        assert_eq!(Shortcut::parse("ctrl+z"), Shortcut::parse("Ctrl+Z"));
        assert!(Shortcut::parse("Hyper+Z").is_none());
        assert!(Shortcut::parse("Ctrl+Nonsense").is_none());
        assert_eq!(Shortcut::parse("Ctrl+0"), Some(Shortcut::new(Modifiers::COMMAND, Key::Num0)));
        assert_eq!(Shortcut::parse("Ctrl++"), Some(Shortcut::new(Modifiers::COMMAND, Key::Plus)));
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
        for id in ["view.zoomIn", "view.zoomOut", "view.zoomReset"] {
            assert!(!Keymap::default().keys(id).is_empty(), "{id} has a key");
        }
        assert!(known.iter().any(|a| a.id == "transport.play"), "commands without parameters are actions");
        assert!(!known.iter().any(|a| a.id == "track.setVolume"), "commands that need parameters are not");
        // The piano roll's tools have their number keys and their own labels.
        for (index, id) in
            ["pianoRoll.tool1", "pianoRoll.tool2", "pianoRoll.tool3", "pianoRoll.tool4", "pianoRoll.tool5"]
                .into_iter()
                .enumerate()
        {
            let action = known.iter().find(|a| a.id == id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(action.group, PIANO_ROLL);
            assert!(piano_roll_only(id), "{id} is read only while the piano roll has the keyboard");
            let number = (index + 1).to_string();
            assert_eq!(Keymap::default().label(id).as_deref(), Some(number.as_str()), "{id}");
        }
    }

    #[test]
    fn the_presets_set_their_keys_and_are_kept_in_the_file() {
        for preset in Preset::ALL {
            let mut keymap = Keymap::default();
            keymap.set_preset(preset);
            assert_eq!(keymap.preset(), preset);
            assert!(keymap.conflicts().is_empty(), "{preset:?} has no key bound twice");
            assert_eq!(keymap.label("selection.duplicate").as_deref(), Some(preset.duplicate_key()));
            assert_eq!(preset.right_click_deletes(), preset == Preset::DrawFirst);
            // Every other key is the default's: the preset only changes what it names.
            for (id, _) in DEFAULTS {
                if id != "selection.duplicate" && id != "view.browser" {
                    assert_eq!(keymap.keys(id), Keymap::default().keys(id), "{id} under {preset:?}");
                }
            }
            assert!(!keymap.keys("view.browser").is_empty(), "{preset:?} leaves the browser a key");
            // The choice comes back from the file.
            let (again, warnings) = Keymap::from_json(&keymap.to_json());
            assert_eq!((&again, warnings.len()), (&keymap, 0));
            assert_eq!(again.preset(), preset);
        }
    }

    #[test]
    fn a_preset_chosen_again_puts_keys_changed_by_hand_back() {
        let mut keymap = Keymap::default();
        keymap.set_preset(Preset::DrawFirst);
        keymap.set("selection.duplicate", vec![Shortcut::new(Modifiers::COMMAND, Key::D)]);
        keymap.set_preset(Preset::DrawFirst);
        assert_eq!(keymap.label("selection.duplicate").as_deref(), Some("Ctrl+B"));
        // And choosing the other preset sets its own keys.
        keymap.set_preset(Preset::SelectFirst);
        assert_eq!(keymap.label("selection.duplicate").as_deref(), Some("Ctrl+D"));
        assert_eq!(keymap.label("view.browser").as_deref(), Some("Ctrl+B"));
    }

    #[test]
    fn an_unknown_or_malformed_preset_in_the_file_falls_back_with_a_warning() {
        let (keymap, warnings) = Keymap::from_json(r#"{ "preset": "Fancy-first", "edit.undo": ["Alt+Z"] }"#);
        assert_eq!(keymap.preset(), Preset::SelectFirst, "an unknown preset is the default one");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Fancy-first"), "{}", warnings[0]);
        assert_eq!(keymap.label("edit.undo").as_deref(), Some("Alt+Z"), "the rest of the file is read");
        // A preset that is not text at all, and a file that is not JSON.
        let (number, warnings) = Keymap::from_json(r#"{ "preset": 3 }"#);
        assert_eq!((number.preset(), warnings.len()), (Preset::SelectFirst, 1));
        let (broken, warnings) = Keymap::from_json("{ not json");
        assert_eq!((broken.preset(), warnings.len()), (Preset::SelectFirst, 1));
        // The name is read without case, as a hand-edited file may write it.
        let (named, warnings) = Keymap::from_json(r#"{ "preset": "draw-first" }"#);
        assert_eq!((named.preset(), warnings.len()), (Preset::DrawFirst, 0));
    }

    #[test]
    fn conflicts_are_found() {
        let mut keymap = Keymap::default();
        keymap.set("transport.play", vec![Shortcut::new(Modifiers::NONE, Key::Space)]);
        let conflicts = keymap.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1.len(), 2);
        // The selected slot and the piano roll share the arrows: they never work at once.
        assert_eq!(Keymap::default().keys("selection.up"), Keymap::default().keys("pianoRoll.up"));
        assert!(!overlap("selection.up", "pianoRoll.up"));
        assert!(overlap("selection.up", "transport.play"));
    }
}
