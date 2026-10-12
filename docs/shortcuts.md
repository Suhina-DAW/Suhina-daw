---
description: Keyboard shortcuts in the desktop app: the default keys, the keymap file and how to change them
read_when: shortcuts, keys, keymap, keymap.json, rebind, cut copy paste, duplicate, hotkeys
type: howto
status: current
last_verified: 2026-10-12
covers:
  - crates/ui/src/keymap.rs
  - crates/ui/src/settings_view.rs
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

The keys are kept in `keymap.json` beside `settings.txt`, in Loopwerk's folder of the user's app
data (`%APPDATA%\Loopwerk\data` on Windows, `~/.local/share/Loopwerk` on Linux,
`~/Library/Application Support/Loopwerk` on macOS). It maps action ids to lists of keys:

```json
{
  "transport.toggle": ["Space"],
  "edit.redo": ["Ctrl+Shift+Z", "Ctrl+Y"],
  "selection.delete": []
}
```

A key is its modifiers and its name joined with `+`: `Ctrl` (Cmd on macOS), `Shift`, `Alt`, then
a key such as `Z`, `Space`, `Delete`, `Backspace`, `Tab`, `Comma`, `F4`. An action the file lists
takes exactly the keys listed (an empty list unbinds it); actions it leaves out keep their default
keys. Unknown actions and keys are skipped with a warning in the status bar.

## Default keys

| Action | Keys |
|---|---|
| Play or stop (`transport.toggle`; Stop goes back to the play start marker or stays, as Settings › General › Stop says) | Space |
| Switch between Session and Arrangement (`view.toggle`) | Tab |
| Show or hide the detail area (`view.detail`) | Shift+Tab |
| Open or close Settings (`settings.open`) | Ctrl+, |
| Undo, redo (`edit.undo`, `edit.redo`) | Ctrl+Z; Ctrl+Shift+Z, Ctrl+Y |
| Copy, cut, paste the selected clip (`selection.copy`, `.cut`, `.paste`) | Ctrl+C, Ctrl+X, Ctrl+V |
| Duplicate, delete the selected clip (`selection.duplicate`, `.delete`) | Ctrl+D; Delete, Backspace |
| Group the selected tracks (`selection.group`) | Ctrl+G |
| Next, previous project (`projects.next`, `projects.previous`) | Ctrl+Tab, Ctrl+Shift+Tab |

Paste places the copy right after the selected clip, or at the playhead on the selected track; it
replaces what it covers, like a moved clip ([arrangement editing](spec/arrangement-editing.md)).
