---
description: Keyboard shortcuts in the desktop app: the default keys, the keymap presets, the keymap file and how to change them
read_when: shortcuts, keys, keymap, keymap.json, keymap preset, Draw-first, Select-first, rebind, cut copy paste, duplicate, hotkeys, quantise, Ctrl+U, piano roll keys, tool keys, number keys choose a tool, arrow keys, select all
type: howto
status: current
last_verified: 2026-10-15
covers:
  - crates/ui/src/keymap.rs
  - crates/ui/src/settings_view.rs
  - crates/ui/src/browser_view.rs
  - apps/suhina/src/main.rs
---

# Keyboard shortcuts

Every key in the desktop app runs an **action**, and which keys run which action is the user's to
change. An action is one of:

- something the window does itself (switch views, open Settings, play or stop);
- something done to the selection, which the window turns into a command (copy the selected clip
  becomes `clip.copy` with that clip);
- any command that needs no parameters (`transport.play`, `edit.undo`, `launcher.stopAll`,
  `projects.next` …), the same commands the CLI and MCP run.

Keys go to the actions, not to moving between buttons: Tab and the arrow keys never move the
keyboard focus from widget to widget (a button holding it would also make every shortcut wait).
Only a text field keeps the keys while it is being typed in.

## Changing keys

Options › Settings › **Shortcuts** lists every action with its keys. **Change** waits for the next
key pressed, with its modifiers, and makes it the action's key (Escape cancels); **Clear** unbinds
the action; **Reset all** goes back to the defaults. A key bound to two actions is shown in red
above the list.

## Presets

The **Preset** row above the list offers two sets of keys, for two ways of working. Choosing one
sets the defaults of the keys it names and leaves every other key as it is; single keys can still
be changed afterwards, and **Reset all** goes back to the preset the keymap was on.

| Preset | For | What it changes |
|---|---|---|
| **Select-first** (default) | selecting and moving notes first | duplicate is **Ctrl+D**; a right-click on a note in the piano roll does nothing |
| **Draw-first** | drawing notes first | duplicate is **Ctrl+B**, and the browser gives it up for **Ctrl+Shift+B**; in the piano roll's Draw and Paint tools a right-click on a note deletes it |

The names describe the habit they suit. The choice is kept with the keys (see below) and read
back when Suhina starts; a `preset` naming something Suhina does not know leaves Select-first and
says so in the status bar.

## The keymap file

The keys are kept in `keymap.json` beside `settings.txt`, in Suhina's folder of the user's app
data (`%APPDATA%\Suhina\data` on Windows, `~/.local/share/suhina` on Linux,
`~/Library/Application Support/Suhina` on macOS). It maps action ids to lists of keys, and names
the preset they came from:

```json
{
  "preset": "Select-first",
  "transport.toggle": ["Space"],
  "edit.redo": ["Ctrl+Shift+Z", "Ctrl+Y"],
  "selection.delete": []
}
```

A key is its modifiers and its name joined with `+`: `Ctrl` (Cmd on macOS), `Shift`, `Alt`, then
a key such as `Z`, `Space`, `Delete`, `Backspace`, `Tab`, `Comma`, `Plus`, `Minus`, `0`, `F4`. An action the file lists
takes exactly the keys listed (an empty list unbinds it); actions it leaves out keep their default
keys. Unknown actions, keys and presets are skipped with a warning in the status bar.

## Default keys

| Action | Keys |
|---|---|
| Play or stop (`transport.toggle`; Play starts from the play start marker or the selected clip, as Settings › General › Play from says; Stop goes back to the marker or stays, as Settings › General › Stop says) | Space |
| Play from the start of the clip selected in the Arrangement view, whatever the setting (`transport.playFromSelection`; from the marker when no clip is selected) | Shift+Space |
| Switch between Session and Arrangement (`view.toggle`) | Tab |
| Show or hide the detail area (`view.detail`) | Shift+Tab |
| Show or hide the browser (`view.browser`) | Ctrl+B |
| Show or hide the sample map in the main area (`view.sampleMap`; Escape on the map closes it too) | Ctrl+Shift+M |
| Open the sample map in a window of its own, or put it back (`view.sampleMapWindow`; [spec](spec/sample-map.md) › Where the map is shown) | Ctrl+Alt+M |
| Search in the browser (`browser.search`; shows it when hidden). In the search field: Up and Down move through the results, Enter adds the selection to the selected track, Escape clears the search and then leaves the field | Ctrl+F |
| Open or close Settings (`settings.open`) | Ctrl+, |
| Make the whole window larger, smaller, its normal size (`view.zoomIn`, `view.zoomOut`, `view.zoomReset`; 50–200 % in 10 % steps, kept in `settings.txt`, [spec](spec/window-zoom.md)) | Ctrl+Plus, Ctrl+Equals; Ctrl+Minus; Ctrl+0 |
| Undo, redo (`edit.undo`, `edit.redo`) | Ctrl+Z; Ctrl+Shift+Z, Ctrl+Y |
| Copy, cut, paste the selected clip (`selection.copy`, `.cut`, `.paste`) | Ctrl+C, Ctrl+X, Ctrl+V |
| Duplicate, delete the selected clip (`selection.duplicate`, `.delete`); in the piano roll, after a click in it, Delete deletes the selected notes instead. The duplicate key is Ctrl+D, or Ctrl+B with the Draw-first preset | Ctrl+D; Delete, Backspace |
| In the piano roll, after a click in it: copy, cut, paste, duplicate the selected notes (the same `selection.*` keys; with no note selected, or nothing copied for a paste, they act on the clip) | Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+D (Ctrl+B with Draw-first) |
| In the piano roll: choose the Select, Draw, Paint, Erase, Slice tool (`pianoRoll.tool1` … `.tool5`, [spec](spec/ui-clip-editor.md) › Drawing notes) | 1, 2, 3, 4, 5 |
| In the piano roll: select every note, clear the selection (`pianoRoll.selectAll`, `.deselect`) | Ctrl+A; Escape |
| In the piano roll: move the selected notes one grid step earlier, later (`pianoRoll.earlier`, `.later`) | Left; Right |
| In the piano roll: move the selected notes a semitone, an octave up or down (`pianoRoll.up`, `.down`, `.octaveUp`, `.octaveDown`) | Up, Down; Shift+Up, Shift+Down |
| In the piano roll: make the selected notes a grid step shorter, longer (`pianoRoll.shorter`, `.longer`; never shorter than a step) | Shift+Left; Shift+Right |
| In the piano roll: switch between the Select and Draw tools (`pianoRoll.draw`, [spec](spec/ui-clip-editor.md) › Drawing notes) | B |
| In the piano roll: play a key of the key column through the track's instrument while it is held; Ctrl+click, Ctrl+drag select the notes of that pitch or those pitches ([spec](spec/ui-clip-editor.md) › Playing the keys) | click, drag on the key column; Ctrl+click, Ctrl+drag |
| Group the selected tracks (`selection.group`) | Ctrl+G |
| In the Session view, with a clip slot selected: launch it (`selection.launch`); select the slot above, below, in the track to the left or right (`selection.up`, `.down`, `.left`, `.right`; [spec](spec/ui-session-view.md) › Launching and selecting) | Enter; Up, Down, Left, Right |
| Quantise the piano roll's selected notes, or all its notes, with the settings the quantise block in the clip column keeps (`pianoRoll.quantize`, [spec](spec/ui-clip-editor.md) › Quantise) | Ctrl+U |
| Add an arrangement marker at the playhead, or at the play start marker while stopped (`marker.addAtPlayhead`; [spec](spec/arrangement-editing.md#markers)) | M |
| Turn arrangement looping on or off; on without a loop region makes one over the first 4 bars (`transport.setLoopEnabled`; [spec](spec/arrangement-editing.md#loop-region)) | Ctrl+L |
| Next, previous project (`projects.next`, `projects.previous`) | Ctrl+Tab, Ctrl+Shift+Tab |
| New project or open one (`file.new`), open one (`file.open`) | Ctrl+T, Ctrl+O |
| Save, save as (`file.save`, `file.saveAs`); save as template (`file.saveTemplate`) | Ctrl+S, Ctrl+Shift+S; none |
| Show which version of Suhina this is (`help.about`, Help › About Suhina, [spec](spec/about.md)) | none |

Copy, cut and paste are Ctrl+C, Ctrl+X and Ctrl+V on Windows and Linux and Cmd+C, Cmd+X and
Cmd+V on macOS. The window reports them, and the other system keys for the same thing (Shift+Delete,
Ctrl+Insert and Shift+Insert on Windows, and the Cut, Copy and Paste keys some keyboards have), as
cut, copy and paste rather than as keys; the keymap reads them as Ctrl+X, Ctrl+C and Ctrl+V, so
rebinding `selection.copy` and the others still works. A paste arrives only while the system
clipboard holds text, so copying a clip or notes in Suhina also puts a short line there.

Paste places the copy right after the selected clip, or at the playhead on the selected track; like a
moved clip, it replaces what it covers unless the stacking setting for its kind is on
([arrangement editing](spec/arrangement-editing.md)). Duplicate does the same.

The piano roll's own keys (`pianoRoll.*`) work only after a click in it; anywhere else Escape and
the arrows keep their other uses (the sample map, the browser's search), and the number keys
(1 to 5) with them. In the piano roll, paste
puts the copied notes right after the selected notes, or at the playhead when it is inside the
clip, else right after where they were copied from; pasted and duplicated notes are selected
([clip editor spec](spec/ui-clip-editor.md) › Shortcuts).

In the Session view, copy and paste act on the selected clip slot (the one clicked last): Ctrl+C
copies its clip, Ctrl+V puts the copied clip in it, replacing what it holds. That is how an
arrangement clip goes into a slot: Ctrl+C on the clip, Tab, click a slot, Ctrl+V
([Session view](spec/ui-session-view.md) › Between the views). Cut, duplicate and delete act on
arrangement clips only.

Zoom in has two keys, `Ctrl+Plus` and `Ctrl+Equals`. On macOS `+` is typed as Shift and `=`, and
while Cmd is held the window reports that key as `=`, never `+`: `Ctrl+Plus` cannot fire there, and
`Cmd+=`, the usual zoom-in key on a Mac, does. Both work on Windows and Linux, and a keyboard with a
key of its own for `+` uses `Ctrl+Plus`.
