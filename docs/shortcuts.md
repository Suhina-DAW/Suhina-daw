---
description: Keyboard shortcuts in the desktop app: the default keys, the keymap file and how to change them
read_when: shortcuts, keys, keymap, keymap.json, rebind, cut copy paste, duplicate, hotkeys, quantise, Ctrl+U, piano roll keys, arrow keys, select all
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

## The keymap file

The keys are kept in `keymap.json` beside `settings.txt`, in Suhina's folder of the user's app
data (`%APPDATA%\Suhina\data` on Windows, `~/.local/share/suhina` on Linux,
`~/Library/Application Support/Suhina` on macOS). It maps action ids to lists of keys:

```json
{
  "transport.toggle": ["Space"],
  "edit.redo": ["Ctrl+Shift+Z", "Ctrl+Y"],
  "selection.delete": []
}
```

A key is its modifiers and its name joined with `+`: `Ctrl` (Cmd on macOS), `Shift`, `Alt`, then
a key such as `Z`, `Space`, `Delete`, `Backspace`, `Tab`, `Comma`, `Plus`, `Minus`, `0`, `F4`. An action the file lists
takes exactly the keys listed (an empty list unbinds it); actions it leaves out keep their default
keys. Unknown actions and keys are skipped with a warning in the status bar.

## Default keys

| Action | Keys |
|---|---|
| Play or stop (`transport.toggle`; Stop goes back to the play start marker or stays, as Settings › General › Stop says) | Space |
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
| Duplicate, delete the selected clip (`selection.duplicate`, `.delete`); in the piano roll, after a click in it, Delete deletes the selected notes instead | Ctrl+D; Delete, Backspace |
| In the piano roll, after a click in it: copy, cut, paste, duplicate the selected notes (the same `selection.*` keys; with no note selected, or nothing copied for a paste, they act on the clip) | Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+D |
| In the piano roll: select every note, clear the selection (`pianoRoll.selectAll`, `.deselect`) | Ctrl+A; Escape |
| In the piano roll: move the selected notes one grid step earlier, later (`pianoRoll.earlier`, `.later`) | Left; Right |
| In the piano roll: move the selected notes a semitone, an octave up or down (`pianoRoll.up`, `.down`, `.octaveUp`, `.octaveDown`) | Up, Down; Shift+Up, Shift+Down |
| In the piano roll: make the selected notes a grid step shorter, longer (`pianoRoll.shorter`, `.longer`; never shorter than a step) | Shift+Left; Shift+Right |
| In the piano roll: switch between the Select and Draw tools (`pianoRoll.draw`, [spec](spec/ui-clip-editor.md) › Drawing notes) | B |
| Group the selected tracks (`selection.group`) | Ctrl+G |
| Quantise the piano roll's selected notes, or all its notes, with the Quantise panel's settings (`pianoRoll.quantize`, [spec](spec/ui-clip-editor.md) › Quantise) | Ctrl+U |
| Next, previous project (`projects.next`, `projects.previous`) | Ctrl+Tab, Ctrl+Shift+Tab |
| New project or open one (`file.new`), open one (`file.open`) | Ctrl+T, Ctrl+O |
| Save, save as (`file.save`, `file.saveAs`); save as template (`file.saveTemplate`) | Ctrl+S, Ctrl+Shift+S; none |

Paste places the copy right after the selected clip, or at the playhead on the selected track; like a
moved clip, it replaces what it covers unless the stacking setting for its kind is on
([arrangement editing](spec/arrangement-editing.md)). Duplicate does the same.

The piano roll's own keys (`pianoRoll.*`) work only after a click in it; anywhere else Escape and
the arrows keep their other uses (the sample map, the browser's search). In the piano roll, paste
puts the copied notes right after the selected notes, or at the playhead when it is inside the
clip, else right after where they were copied from; pasted and duplicated notes are selected
([clip editor spec](spec/ui-clip-editor.md) › Shortcuts).

Zoom in has two keys, `Ctrl+Plus` and `Ctrl+Equals`. On macOS `+` is typed as Shift and `=`, and
while Cmd is held the window reports that key as `=`, never `+`: `Ctrl+Plus` cannot fire there, and
`Cmd+=`, the usual zoom-in key on a Mac, does. Both work on Windows and Linux, and a keyboard with a
key of its own for `+` uses `Ctrl+Plus`.
