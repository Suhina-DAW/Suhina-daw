---
description: Functional spec of the MIDI clip editor (piano roll): layout, grid and snapping, zoom, drawing, selecting, moving, resizing, velocity, quantise, scales, ghost notes, slicing, chords, shortcuts, and the note.* commands
read_when: piano roll, clip editor, MIDI editor, note editing, draw notes, select notes, move notes, resize notes, velocity lane, velocity, grid, snapping, Alt off-grid, zoom, detail area height, scale highlighting, fold, ghost notes, quantise, quantize, slice, glue, chords, strum, note.add, note.delete, note.move, note.resize, note.setVelocity, note.list
type: spec
status: draft
last_verified: 2026-10-15
covers:
  - crates/ui/src/piano_roll.rs
  - crates/session/src/notes.rs
---

# UI: clip editor (MIDI piano roll)

Functional spec of the editor that opens in the detail area for a MIDI clip: a piano roll with a
keyboard on the left, a note grid, and a velocity lane below. Only structure and behaviour; the look
comes from [`docs/design/brief.md`](../design/brief.md) and the measurements from
[`docs/design/components.md`](../design/components.md) › Clip editor.

**Sources.** Written in our own words from public user manuals, following the clean-room rules
(`CONTRIBUTING.md` › Clean-room): the chapter on editing MIDI in the Ableton Live 12 reference
manual, the piano roll pages of the FL Studio online manual, and the note editor chapters of the
Bitwig Studio and REAPER user guides; and from using Live 12.4.6. Nothing here comes from a
product's code, files or artwork. Where the products differ, the spec picks one behaviour and says
why. Items marked **(to observe)** still need checking against the program.

**What is built.** Each section says which parts the first version builds (**Built**) and which
come later (**Later**, with the issue that tracks it). The first version is
[issue #200](https://github.com/suhina-daw/suhina-daw/issues/200)'s first pull request: a piano roll
for a selected arrangement clip, drawing, selecting, moving, resizing and deleting notes, a velocity
lane, the grid and zoom, and the `note.*` commands.

## Where it opens

- **Built:** selecting a MIDI clip in the Arrangement view shows the clip's properties column on the
  left of the detail area and the piano roll in the rest of it, in place of the track's devices.
  Selecting a track, or an audio clip, shows what it showed before (devices; an audio clip's
  properties). With the detail area stacked on the right, the piano roll sits under the properties.
- **Later ([#343](https://github.com/suhina-daw/suhina-daw/issues/343)):** a clip selected in a Session slot opens it too, once slot selection lands (open pull
  request #302); the piano roll then edits that slot's clip with the same commands (`scene`
  instead of `index`). A double-click on a clip opens the detail area if it is closed.
- **Later ([#64](https://github.com/suhina-daw/suhina-daw/issues/64)):** the device view and the
  clip editor as two tabs or a toggle, so a MIDI clip's track devices stay one click away (Shift+Tab
  in the familiar workflow).

## Coordinates

- **Time** is in beats from the start of the clip's content (its first beat is 0), the same beats
  the model stores (`Note::time`, `Note::duration`). The ruler shows bars and beats of the content
  (bar 1 at beat 0), in the project's time signature.
- **Pitch** is the MIDI key, 0 to 127. Key names use the convention of naming MIDI 60 **C3**, so
  the lowest key is C-2 and the highest G8. Every C is labelled.
- **Velocity** is shown and edited as 1 to 127 (MIDI). The model keeps 0.0 to 1.0; a command's
  velocity v is stored as v / 127, and shown as the stored value × 127, rounded. 0 is not a
  velocity (it means a note-off in MIDI).
- A clip shares its content with every clip that plays the same content (a duplicated clip, a
  shared pattern, ADR 0001). Editing the notes edits the content, so every clip that plays it
  changes. **Later ([#352](https://github.com/suhina-daw/suhina-daw/issues/352)):** a duplicated clip gets its own content when its notes are first
  edited, so it behaves like an independent copy, unless the user asked for a linked copy.

## Layout

From left to right: the clip properties column (exists), the MIDI tools column (later), the editor.
The editor, top to bottom:

1. **Toolbar** (later): tabs for Notes, Envelopes and Expression; Fold; Scale with the scale name;
   the grid value.
2. **Ruler** (built): bar numbers and bar lines. **Later ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)):** the
   clip's loop region with draggable start and end, the clip's start and end markers.
3. **Key column and note grid** (built): one row per key, black-key rows darker than white-key
   rows, a stronger line under every C; bar lines stronger than grid lines. The editor spans the
   part of the content the clip plays: to the loop's end when the clip loops, else to its offset
   plus its length; the grid past that is shaded. Notes are drawn in the track colour, their opacity rising
   with velocity; a selected note has a light outline; a note wide enough shows its name.
4. **Velocity lane** (built): under the grid, sharing its time axis, 72 px tall (less in a short
   detail area: at most 30 % of its height). One stem per note at the
   note's start, its height the velocity; a selected stem is brighter. **Later:** the lane's
   height dragged by its top edge; chord notes on the same start drawn side by side, a few pixels
   apart, so each can be grabbed; the lane's tools (randomise, ramp, deviation) ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)).

Everything scales with the detail area: the detail area's splitter (exists) makes the editor taller
or shorter, and the key rows and the velocity lane keep their own heights while the visible range
of keys grows and shrinks.

## Grid and snapping

- **Built:** the grid follows the grid value chosen in the control bar (the launch quantisation
  menu: 1 bar, 1/4, 1/16, 1/8T and so on; `launcher.setQuantization`). The same value draws the
  vertical lines and is the step that notes snap to. When lines would be closer than a few pixels,
  every second line is left out until they are far enough apart (snapping uses the same, coarser
  step, so what snaps is what is drawn). `none` draws beats and bars and does not snap.
- **Built:** starts and ends of drawn, moved and resized notes snap to the grid. Holding **Alt**
  while drawing or dragging places them freely (no snapping). A move keeps a note's distance to the
  grid: the drag snaps the **offset** to whole grid steps, so a note that was off the grid stays
  just as far off (the familiar behaviour; it keeps a played-in groove).
- **Later ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)):** the piano roll's own grid value, independent of the launch quantisation, with an
  adaptive mode that follows the zoom (finer when zoomed in), and triplet and off toggles; keyboard
  shortcuts to make the grid finer or coarser (Ctrl+1 and Ctrl+2 in the familiar workflow) and to
  toggle snapping (Ctrl+4) and triplets (Ctrl+3). Once the Arrangement view's grid
  (pull request #320) lands, the piano roll uses its line logic.
- **Later ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)):** "magnetic" snapping to other notes' starts and ends within a few pixels.

## Zoom and scroll

- **Built:** the mouse wheel scrolls up and down through keys; **Shift**+wheel scrolls in time;
  **Ctrl**+wheel zooms time around the pointer; **Ctrl+Shift**+wheel zooms key height around the
  pointer. Dragging with the middle mouse button pans both ways. A newly shown clip opens scrolled
  so its notes are in view (the middle of its lowest and highest note; C3 for an empty clip) and
  zoomed so the whole clip fits the width.
- **Built:** key rows are 6 to 32 px tall (default 12 px); time is 4 to 400 px per beat.
- **Later ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)):** dragging up and down in the ruler zooms time, sideways scrolls it (the familiar
  ruler gesture); zoom to fit the selection, or the whole clip (Z and Shift+Z in the familiar
  workflow) **(to observe: exact keys)**; dragging the key column zooms key height; Fold (show only
  the keys that have notes) and Fold to scale.

## Drawing notes

- **Built:** a **double-click** on an empty spot of the grid adds a note there: its start snapped
  down to the grid line before the pointer (Alt: where the pointer is), its key the row under the
  pointer, its length one grid step (a quarter beat with `none`), velocity 100. The new note is
  selected. A double-click on a note deletes it.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** a draw mode (B toggles it, or holding Ctrl or Cmd in other products): a click adds a
  note, and dragging sideways while drawing paints notes one grid step apart in the same row;
  dragging up or down while drawing sets the length. A new note takes the length and velocity of
  the note last edited (the FL Studio habit), so repeated drawing keeps one feel.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** clicking a key in the key column plays it (audition, through the track's instrument)
  and selects every note of that key; while notes are drawn, moved or transposed they sound once
  (preview, with a toggle in the toolbar).

## Selecting

- **Built:** a click on a note selects it alone; **Shift**+click (or Ctrl+click) adds it to the
  selection or takes it out. A click on an empty spot of the grid clears the selection. Dragging
  from an empty spot draws a rectangle; when it is let go, the notes it touches are selected
  (with Shift, added to the selection).
- **Built:** the selection lives in the window only; it is not part of the project and is not an
  undo step. It is cleared when another clip is shown.
- **Later ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** Ctrl+A selects every note; a click on the ruler selects a time range, and every note
  starting in it; Shift+arrows extend; a lasso; "select all notes of the same key"; Escape clears.

## Moving

- **Built:** dragging a selected note (or any note, which selects it first unless it is already in
  the selection) moves the whole selection: sideways in time, snapped by whole grid steps (Alt:
  freely), up and down by semitones. The notes are shown where they go while dragging; the move is
  one command, one undo step, when the mouse is let go. A move that would put a note before the
  content's start or outside keys 0 to 127 stops at that limit, for the whole selection, so the
  selection keeps its shape.
- **Later ([#344](https://github.com/suhina-daw/suhina-daw/issues/344), [#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** the arrow keys move the selection by one grid step (left and right) or a semitone
  (up and down); Shift+up and down by an octave. Ctrl+drag (Alt in some products) copies the notes
  instead of moving them. Moving notes past the clip's end extends the clip **(to observe)**.

## Resizing

- **Built:** the pointer over the last few pixels of a note shows a resize cursor; dragging there
  changes the length of every selected note by the same amount, the dragged note's end snapping to
  the grid (Alt: freely). Dragged onto or before its own start, the dragged note's end goes to the
  first grid line after its start; no selected note gets shorter than a hundredth of a beat.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351), [#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** dragging a note's left edge moves its start and keeps its end; Shift+left and right
  arrows change the selection's length by one grid step; a modifier while resizing stretches the
  selection in time around its start (scale the positions as well as the lengths).

## Deleting

- **Built:** **Delete** or **Backspace** while the piano roll has the keyboard (after a click in
  it) deletes the selected notes, one undo step. Without a note selected, those keys keep doing
  what they do elsewhere (delete the selected clip). A double-click on a note deletes it.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351), [#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** right-click on a note (or a drag with the right button) deletes notes, the FL Studio
  habit, as an option in Settings; cut, copy, paste and duplicate of notes (Ctrl+X, C, V, D),
  pasting at the playhead or after the selection.

## Velocity

- **Built:** in the velocity lane, dragging a stem up or down changes its velocity, and with
  several notes selected (and the dragged one among them), all of them change by the same amount,
  each kept within 1 to 127. The lane shows the value while dragging; it is one undo step.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** a note's velocity changed by dragging on the note itself with a modifier (Alt+drag up
  and down in the familiar workflow); drawing a line across the lane to set many stems at once;
  release velocity and a note's probability and velocity range (the per-note chance and
  variation newer products offer), shown as a second lane; randomise, ramp and deviation tools.

## Quantise

- **Later ([#345](https://github.com/suhina-daw/suhina-daw/issues/345)):** quantise the selection (or every note) to the grid: Ctrl+U applies the last
  settings, a dialog sets the grid, whether starts and or ends move, and how much (an amount in
  percent, 100 % lands on the grid). Quantise is a command (`note.quantize`) with the grid,
  amount and which edges as parameters, so it also runs from the CLI and MCP. Quantise while
  recording (record quantisation) belongs to recording, which has its own issue.

## Scale and key

- **Later ([#347](https://github.com/suhina-daw/suhina-daw/issues/347)):** each clip has a scale (root and mode; where the model keeps it is decided with
  [ADR 0001](../decisions/0001-daw-neutral-model.md)); the piano roll marks the keys in the scale (lighter rows, a mark on the key column) and can fold to
  them. A note drawn with "snap to scale" on lands on the nearest scale key. The project can set
  a default scale that new clips take. Fit to scale moves notes outside the scale to the nearest
  key in it.

## Ghost notes

- **Later ([#348](https://github.com/suhina-daw/suhina-daw/issues/348)):** the notes of other clips on the same track, and optionally of one other clip the user
  picks (another track), are drawn faintly behind the clip's own notes and cannot be edited; this
  is how a bass line is written against the chords. Which clips are shown is a toolbar menu
  (none, this track, a chosen clip). Clips that play at the same time in the arrangement are the
  default **(to observe in the products that offer it)**.

## Slicing and gluing

- **Later ([#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** split notes at the grid lines they cross (a "slice" tool: a cut drawn across notes
  splits each where the line crosses it, snapping to the grid; or a command that chops the
  selection into notes of one grid step each); glue (join) touching notes of the same key into one
  note (Ctrl+J in the familiar workflow); legato (each note lasts until the next one starts);
  Commands `note.split` and `note.join`.

## Chords

- **Later ([#350](https://github.com/suhina-daw/suhina-daw/issues/350)):** a chord tool: draw one note and a chosen chord (major, minor, seventh and so on, or
  one from the clip's scale) is added on top of it. Strum and arpeggiate a selected chord: spread
  the starts of its notes over a time set by dragging (strum), or play them one after another on
  the grid (arpeggiate). Inversions move the lowest or highest note an octave.

## Other note tools (MIDI tools column)

- **Later ([#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** transpose by a number of semitones (a field), ×2 and ÷2 (stretch the selection or the
  clip in time), reverse, invert (mirror pitches), humanise (small random moves of start and
  velocity), set length. Each is a command over the selection, one undo step.

## Shortcuts

Built in the first version:

| Keys | Does |
|---|---|
| Double-click on an empty spot | Adds a note |
| Double-click on a note | Deletes it |
| Click, Shift+click, Ctrl+click | Selects a note, adds or takes it out of the selection |
| Drag from an empty spot | Selects the notes in the rectangle |
| Drag a note | Moves the selection (time snapped by grid steps, pitch by semitones) |
| Drag a note's right edge | Changes the selection's length |
| Alt while drawing or dragging | No snapping |
| Drag a stem in the velocity lane | Changes the selection's velocity |
| Delete, Backspace | Deletes the selected notes (after a click in the piano roll) |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo (as everywhere) |
| Wheel, Shift+wheel | Scroll keys, scroll time |
| Ctrl+wheel, Ctrl+Shift+wheel | Zoom time, zoom key height |
| Middle-button drag | Pan |

Later: arrow keys, Ctrl+A, Ctrl+D, copy and paste, B (draw mode), Ctrl+U (quantise), Ctrl+J (glue),
Ctrl+1 to Ctrl+4 (grid), Z (zoom to selection). They are added to the keymap
([`docs/shortcuts.md`](../shortcuts.md)) when built, so they can be changed like the rest.

## Commands

Every edit is a command (AGENTS.md rule 6), so the piano roll, the CLI and MCP make the same
edits. A command names a clip by `track` and either `index` (an arrangement clip, as
`project.clips` lists them) or `scene` (the clip in the track's slot of that scene, from 1). The
clip must be a MIDI clip: an audio clip, an empty slot or a clip index that does not exist is an
error. Notes are named by their **index** in the clip's notes, as `note.list` returns them; edits
keep the order (`note.add` appends, `note.delete` removes and the later notes move up), so the
indices of notes an edit did not delete stay valid. Each command is one undo step and changes
nothing when it fails.

| Command | Params | Does |
|---|---|---|
| `note.list` | `track`, `index` or `scene` | The clip's notes: index, time, length, key, velocity (1 to 127) |
| `note.add` | clip, `notes`: a list of `{time, length, key, velocity?}` | Adds notes (velocity 100 when left out); returns their indices |
| `note.delete` | clip, `notes`: a list of indices | Deletes them |
| `note.move` | clip, `notes`, `beats?`, `semitones?` | Moves them in time and in pitch |
| `note.resize` | clip, `notes`, `length?` or `by?` | Sets their length, or changes it by `by` beats |
| `note.setVelocity` | clip, `notes`, `velocity?` or `by?` | Sets their velocity, or changes it by `by` (each kept within 1 to 127) |

Limits, so a hostile input cannot break the project or exhaust memory: time from 0 to 120 000 beats
(the engine's furthest start), length above 0 and at most 120 000 beats, keys 0 to 127, velocity 1
to 127 for a set value, at most 10 000 notes named or added in one command and 100 000 notes in a
clip. A move or resize that would take any note out of these limits fails as a whole; the piano
roll limits its drags beforehand so a drag never fails. A list of indices that repeats one counts
it once.

## Open

- Envelopes and Expression tabs (per-note pitch bend, pressure, timbre; clip automation): a
  separate spec.
- Audio clip editor (sample view, warp markers): a separate spec.
- MIDI recording into a clip, overdub and step input: belongs to recording.
