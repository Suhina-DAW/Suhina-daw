---
description: Functional spec of the MIDI clip editor (piano roll): layout, toolbar size and icons, the six tools and the Mute tool, the MIDI tools column, grid and snapping, zoom, drawing, selecting, moving, resizing, velocity, quantise, scales, ghost notes, slicing, a playable key column, chords, shortcuts and keymap presets, and the note.* and preview.* commands
read_when: piano roll, clip editor, toolbar, toolbar size, toolbar icons, tool icons, draw tool, paint tool, erase tool, slice tool, mute tool, muted note, note.setMuted, MIDI tools column, transpose, humanise, humanize, fit to scale, invert, reverse, legato, stretch, randomise velocity, ramp velocity, B key, number keys choose a tool, right button and wheel, Shift and Draw, pulled to the top, maximise piano roll, piano roll grid value, grid menu, adaptive grid, fixed grid, snap to grid, snap off, triplet grid, narrow grid, widen grid, Ctrl+1, Ctrl+2, Ctrl+3, Ctrl+4, MIDI editor, note editing, draw notes, select notes, move notes, resize notes, velocity lane, velocity, grid, snapping, Alt off-grid, zoom, detail area height, scale highlighting, fold, ghost notes, quantise, quantize, inline quantise, clip column, selection info, slice, glue, chords, strum, playable keyboard, key column plays, preview.noteOn, preview.noteOff, note.add, note.delete, note.move, note.resize, note.setVelocity, note.split, note.list, note.quantize, Ctrl+U, note.copy, note.cut, note.paste, note.duplicate, copy paste notes, arrow keys, select all notes, keymap preset, Draw-first, Select-first, Mouse-first, mouse switches, duplicate key, workflow mode, workflow menu, Custom mode, double-click, double-click adds a note, double-click deletes a note, triple click, least height, short detail area, close the piano roll, hide editor, close button, collapse the detail area, notes in view, centred notes
type: spec
status: draft
last_verified: 2026-10-16
covers:
  - crates/ui/src/piano_roll.rs
  - crates/ui/src/piano_roll_grid.rs
  - crates/ui/src/midi_tools.rs
  - crates/ui/src/detail_view.rs
  - crates/session/src/notes.rs
  - crates/session/src/note_tools.rs
  - crates/session/src/preview.rs
---

# UI: clip editor (MIDI piano roll)

Functional spec of the editor that opens in the detail area for a MIDI clip: a piano roll with a
keyboard on the left, a note grid, and a velocity lane below. Only structure and behaviour; the look
comes from [`docs/design/brief.md`](../design/brief.md) and the measurements from
[`docs/design/components.md`](../design/components.md) › Clip editor.

**Sources.** Suhina's own design, written in our own words under the clean-room rules
(`CONTRIBUTING.md` › Clean-room); nothing here comes from another product's code, files or
artwork. Items marked **(to decide)** are open design questions to settle when they are built.

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
  clip editor as two tabs or a toggle, so a MIDI clip's track devices stay one click away.

## Coordinates

- **Time** is in beats from the start of the clip's content (its first beat is 0), the same beats
  the model stores (`Note::time`, `Note::duration`). The ruler shows bars and beats of the content
  (bar 1 at beat 0), in the project's time signature.
- **Pitch** is the MIDI key, 0 to 127. Key names use the convention of naming MIDI 60 **C3**, so
  the lowest key is C-2 and the highest G8. Every C is labelled.
- **Velocity** is shown and edited as 1 to 127 (MIDI). The model keeps 0.0 to 1.0; a command's
  velocity v is stored as v / 127, and shown as the stored value × 127, rounded. 0 is not a
  velocity (it means a note-off in MIDI).
- A clip may share its content with other clips (a duplicated clip, a Live Set's clips that
  point at one content, ADR 0001). The first note edit that changes such
  a clip's notes gives that clip its own copy of the content, with the edit, in the same command
  and so the same undo step; the other clips keep the content as it was
  ([#352](https://github.com/suhina-daw/suhina-daw/issues/352)). The copy keeps the content's
  name. An edit that changes nothing, or fails, copies nothing. A shared pattern (a content
  with lanes for named tracks) stays one content: editing it through one placement changes every
  placement, as it is meant to. **Later:** a linked copy the user asks for (a clip that stays
  shared on purpose), shown on the clip.

## Layout

From left to right: the clip properties column (exists), the MIDI tools column (built,
[#460](https://github.com/suhina-daw/suhina-daw/issues/460)), the editor. The editor, top to
bottom:

1. **Toolbar** (built, [#442](https://github.com/suhina-daw/suhina-daw/issues/442), icons
   [#443](https://github.com/suhina-daw/suhina-daw/issues/443) and
   [#460](https://github.com/suhina-daw/suhina-daw/issues/460)): the tools, then the note
   clipboard, then the MIDI tools column's toggle, then the **workflow menu** (› Workflow
   modes), and at the right end the piano roll's grid
   value (› Grid and snapping), and after it, at the very end, the **×** that hides the editor
   (› Least height and closing). **Later:** tabs for Notes, Envelopes and Expression; Fold; Scale
   with the scale name.

   The tools are **Select**, **Draw**, **Paint**, **Erase**, **Slice** and **Mute** (› Drawing
   notes, › Muting), each an
   icon button: the shape is drawn in code with our own strokes ([`crates/ui/src/browser_view.rs`](../../crates/ui/src/browser_view.rs)
   › `icon` does the same for the browser), on a 14 px box, 1.1 px wide, in `text.secondary`; the
   chosen tool's is `text.primary` on `control.activeBackground`. A click chooses the tool; the
   number keys **1** to **6** do too, after a click in the piano roll (› Shortcuts). The pointer
   over a button shows its name and its current key, such as "Draw (2)", and the button has that
   name as its accessible label, so a reader and a test find it by name.

   The six tool shapes are drawn as an outline (the pointer is a filled arrow with a stem, the
   pencil and the brush a tip and a body, the eraser a slanted block with a lighter rubbing end,
   the blade a handle over a line, the mute tool a speaker with a cross beside it), so they are
   told apart at 14 px and at the larger toolbar sizes below. The pointer over the grid takes the
   tool's own shape where the window can show one: a crosshair for Draw and Paint, an **eraser**
   for Erase (the shape of the eraser button, not the no-entry sign it used before), a text cursor
   for Slice, an arrow for Select and Mute.

   **Toolbar size** is a setting, Settings › General › Piano roll › **Toolbar size**: **Normal**
   (100 %), **Large** (125 %, the default) and **Larger** (150 %). It scales the toolbar's height,
   every button's box and the icon strokes inside it together, so the icons stay 1.1 px at Normal
   and grow with the box; the rest of the editor (grid, notes, ruler) does not change with it. The
   toolbar's height is its tallest control either way, and a size too large for the toolbar's
   room is shown at the largest that fits, never clipped.

   After a divider, **Cut**, **Copy**, **Paste** and **Duplicate** icon buttons run what
   Ctrl+X, Ctrl+C, Ctrl+V and the duplicate key run (› Copy, cut, paste and duplicate) on the
   selected notes; they are greyed out (a `text.hint` shape, no highlight, no action) when there
   is nothing to act on: no note selected for Cut, Copy and Duplicate, nothing copied for Paste.
   The **MIDI tools** icon (three horizontal sliders) shows and hides the MIDI tools column
   (› Other note tools); it is highlighted while the column shows, and a click on it is window
   state only, not a command. The grid menu stays at the right end of the toolbar (› Grid and
   snapping); the quantise
   controls are in the clip's properties column (› Quantise).
2. **Ruler** (built): bar numbers and bar lines. **Later ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)):** the
   clip's loop region with draggable start and end, the clip's start and end markers.
3. **Key column and note grid** (built): one row per key, black-key rows darker than white-key
   rows, a stronger line under every C; bar lines stronger than grid lines. The editor spans the
   part of the content the clip plays: to the loop's end when the clip loops, else to its offset
   plus its length; the grid past that is shaded. Notes are drawn in the track colour, their opacity rising
   with velocity; a selected note has a light outline; a note wide enough shows its name.
   **Playhead** (built): while the transport runs, a 1 px line in the accent colour over the
   ruler, the grid and the velocity lane shows where playback is in the clip's content, mapped
   through the clip's start, offset and loop (a looping clip shows it going round its loop), in
   step with the Arrangement view's playhead. Outside the clip, or while stopped, there is none.
4. **Velocity lane** (built): under the grid, sharing its time axis, 72 px tall (less in a short
   detail area: at most 30 % of the room under the ruler, and never so tall that fewer than five
   key rows are left above it; when that leaves it under 12 px it is not drawn at all, and the
   key rows take its room). One stem per note at the
   note's start, its height the velocity; a selected stem is brighter. **Later:** the lane's
   height dragged by its top edge; chord notes on the same start drawn side by side, a few pixels
   apart, so each can be grabbed; the lane's tools (randomise, ramp, deviation) ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)).

Everything scales with the detail area: the detail area's splitter (exists) makes the editor taller
or shorter, and the key rows and the velocity lane keep their own heights while the visible range
of keys grows and shrinks.

### Least height and closing

**Built:** the smallest open piano roll is 120 px of detail area (in a row under the view): the
toolbar, the ruler and at least five key rows at the default 12 px row height, with the clip
column beside them showing its title (the clip's name) and its Track, Start, Length, Loop and
Notes rows whole. The splitter, dragged down, stops at that height. Above it the view keeps its
own least height (160 px), as before; with the detail area showing a track's devices instead the
detail area's least height stays 80 px.

The editor closes three ways, and each does the same as **View › Hide detail area** (Shift+Tab):
the detail area is hidden and the view takes the whole height, its height is kept for when it
comes back. It is window state, not a command, and not an undo step.

- **The splitter dragged down** past half the least height (60 px from the bottom with the
  editor, 40 px with devices): the detail area collapses to the 6 px splitter along the bottom
  of the window. While the same drag goes back up past that point, it opens again under the
  pointer. Once it is let go, the splitter stays at the bottom, and dragging it up opens the
  detail area again at the height the drag reaches (at least the least height).
- **The ×** at the right end of the piano roll's toolbar (its top row), named **Hide editor**,
  with its key in the tooltip.
- **View › Hide editor** (Ctrl+Shift+E, the keymap action `view.editor`), shown while a clip's
  editor is open; while it is hidden and a clip is selected the entry reads **Show editor** and
  opens it. Escape does not close the editor: in the piano roll it already clears the note
  selection (`pianoRoll.deselect`) and closes the quantise block, so the editor has a key of its
  own instead, one no other default uses.

After the editor is closed one of these ways, **selecting a clip** (a click on a clip in the
Arrangement view, another clip, or a slot's clip in the Session view) opens it again at the
height it had. A detail area hidden with Shift+Tab stays hidden when a clip is selected, as
before; Shift+Tab, View › Show detail area and the splitter open it either way. A click on an
empty spot of the Arrangement view clears the clip selection (› [Arrangement
editing](arrangement-editing.md) › Selecting clips); the detail area stays open and shows the
track's devices, or the hint.

### Pulled to the top

**Built ([#442](https://github.com/suhina-daw/suhina-daw/issues/442)):** while the piano roll shows a
clip under the Arrangement view, its splitter can be dragged all the way up. Past the view's least
height (160 px) the piano roll snaps to the top; dragged down again past that height, the view comes
back at the height the drag reaches. A double-click on the splitter takes it to the top and back.
What stays above it is a setting, Settings › General › Piano roll › **Pulled to the top**:

- **Edited track** (default): the Arrangement view keeps its ruler and the lane of the track whose
  clip is edited, lit (a lighter lane, an accent edge, its header shown as selected); no other
  track, no return tracks and no main track. The view's scroll position is kept for when it comes
  back.
- **Whole window**: the piano roll fills the whole area under the control bar.

It stays pulled up while MIDI clips are edited; selecting something else (an audio clip, a track,
nothing), hiding the detail area (Shift+Tab) or the Session view shows the view as before. With the
device chain as a column beside the view (Settings › Interface › Detail area) there is nothing to
pull up.

## Grid and snapping

- **Built ([#442](https://github.com/suhina-daw/suhina-daw/issues/442), [#346](https://github.com/suhina-daw/suhina-daw/issues/346)):**
  the piano roll has its own grid value; it lives in the window, not the project, and is a
  snapping 1/16 until another is picked. The control bar's grid value (the launch quantisation,
  `launcher.setQuantization`) applies to the Arrangement view only. The grid value draws the
  vertical lines and is the step that notes snap to. When lines would be closer than a few pixels,
  every second line is left out until they are far enough apart (snapping uses the same, coarser
  step, so what snaps is what is drawn).
- **Built ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)): the grid menu.** The
  button at the right end of the toolbar, after the word **Grid**, shows the value (`1/16`,
  `1/8T`, `2 bars`, `Adaptive`, `Adaptive T`, with ` off` after it while snapping is off; its
  accessible label is `Grid value` and that text). It opens one menu that holds everything about
  the grid, in this order, each entry with its key on the right (the key the keymap has bound to
  it now, also in the entry's accessible label, such as `Narrow grid (Ctrl+1)`) and the chosen
  ones ticked:
  1. **Snap to grid** (Ctrl+4): on or off. Off, the lines are still drawn and nothing snaps:
     drawing, moving, resizing and slicing place notes freely, as with Alt held, and a new note
     takes the length of the note last drawn or clicked, else a 1/16.
  2. **Adaptive grid**: **Adaptive widest**, **wide**, **medium**, **narrow**, **narrowest**. The
     lines follow the zoom: the step is the finest note value of 1/32, 1/16, 1/8, 1/4 and 1/2
     (the ones shorter than a bar), then 1 bar, 2 bars, 4 bars and on doubling, whose lines are at
     least 96, 48, 24, 12 or 6 points apart (widest to narrowest). Zooming in makes it finer,
     zooming out coarser; it is never finer than 1/32.
  3. **Fixed grid**: **8 bars**, **4 bars**, **2 bars**, **1 bar**, **1/2**, **1/4**, **1/8**,
     **1/16**, **1/32**. A note value is that part of a 4/4 bar (1/4 is a beat); a bar is the time
     signature's bar.
  4. **Narrow grid** (Ctrl+1) and **Widen grid** (Ctrl+2): one fixed value finer or coarser, or
     one adaptive width narrower or wider; the finest and the coarsest stay where they are.
  5. **Triplet grid** (Ctrl+3): on or off. On, every note value is two thirds as long (1/8T is a
     third of a beat), adaptive ones too; bar values stay as they are.

  The keys work after a click in the piano roll, like its other keys (› Shortcuts). The grid value
  and its steps are the `piano_roll_grid` module of the `ui` crate (`crates/ui/src/piano_roll_grid.rs`).
- **Built:** Settings › General › Piano roll › **Piano roll grid** set to **Control bar** makes the
  piano roll use the control bar's value instead, and its menu and keys then set the control
  bar's (`launcher.setQuantization`). The control bar has no adaptive grid and no 1/32 triplet,
  so an entry whose result it cannot hold is greyed out and its key does nothing: the adaptive
  widths always, and Triplet grid or Narrow grid where they would reach a 1/32 triplet. Snap to
  grid off is the control bar's `none` (the button shows `1/16 off`); picking a fixed value snaps
  again.
- **Choice left open:** the issue proposed the adaptive grid as the default; the default stays a
  snapping fixed 1/16, as before the menu, until that is decided.
- **Built:** starts and ends of drawn, moved and resized notes snap to the grid. Holding **Alt**
  while drawing or dragging places them freely (no snapping). A move keeps a note's distance to the
  grid: the drag snaps the **offset** to whole grid steps, so a note that was off the grid stays
  just as far off (it keeps a played-in groove).
- **Later:** the same grid menu in the Arrangement view, for its own grid.
- **Later ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)):** "magnetic" snapping to other notes' starts and ends within a few pixels.

## Zoom and scroll

- **Built:** the mouse wheel scrolls up and down through keys; **Shift**+wheel scrolls in time;
  **Ctrl**+wheel zooms time around the pointer; **Ctrl+Shift**+wheel or **Alt**+wheel (Option on
  macOS, as in the Arrangement view) zooms key height around the pointer. Dragging with the middle mouse button pans both ways. A newly shown clip opens scrolled
  so its notes are in view (the middle of its lowest and highest note; C3 for an empty clip) and
  zoomed so the whole clip fits the width.
- **Built:** the notes stay in view at any height. Until the key rows are scrolled by hand (the
  wheel, a middle-button pan, zooming the key height), every change in the editor's height (the
  splitter, the window, pulling it to the top, closing and opening it) centres the key rows on the
  notes again, so at the least height the five rows show the middle of the clip's notes, not the
  rows around C3 it opened at. Once the key rows have been scrolled by hand they stay where they
  were put until another clip is shown.
- **Built:** while the **right** mouse button is held down over the grid, each wheel step chooses
  the next tool, or with the wheel turned the other way the previous one, in the order Select →
  Draw → Paint → Erase → Slice → Mute, stopping at the first and the last tool (no wrapping round)
  (› Drawing notes for the tools). The wheel never scrolls or zooms while the right button is
  held, nor afterwards with what is left of the wheel's smoothed movement: scrolling comes back
  once the button is up and the wheel has stopped.
  The toolbar's highlight moves to the new tool at once and the pointer takes the new tool's
  cursor. Letting the right button go opens no menu, adds no note and changes no selection.
- **Built:** key rows are 6 to 32 px tall (default 12 px); time is 4 to 400 px per beat.
- **Later ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)):** dragging up and down in the ruler zooms time, sideways scrolls it; zoom to fit
  the selection, or the whole clip (Z and Shift+Z **(to decide)**); dragging the key column zooms
  key height. Fold (show only the keys that have notes) and Fold to scale come with scales.

## Drawing notes

- **Built:** a **double-click** on an empty spot of the grid adds a note there: its start snapped
  down to the grid line before the pointer (Alt: where the pointer is), its key the row under the
  pointer, its length one grid step (a quarter beat with `none`), velocity 100. The new note is
  selected. A double-click on a note deletes it. Both are workflow settings, and what a
  double-click does with every tool and in every part of the editor is › Double-click.
- **Built:** the toolbar's tools ([#442](https://github.com/suhina-daw/suhina-daw/issues/442),
  Slice [#443](https://github.com/suhina-daw/suhina-daw/issues/443)) decide
  what a press in the grid does (the velocity lane works the same with every tool):
  - **Select** (default): as above and in the sections below.
  - **Draw**: a click on an empty spot adds a note there; pressing and dragging sideways sets its
    length, its end on the grid (Alt: free), at least one grid step. A click on a note selects it,
    and dragging a note moves or resizes it as with Select. A double-click does nothing more.
  - **Draw with Shift held** draws notes **back to back**: the drag stays in the row it started in,
    and the notes it draws follow one another from the press position, each as long as the note
    last drawn or clicked (one grid step until then), the first starting where the press was
    (snapped; Alt: where the pointer is). The drag's end decides when to stop: notes are laid down
    while their end is before it, the last one as long as the others. **Draw with Shift and Paint
    differ in two ways:** Shift+Draw uses the last note's length while Paint uses the length of
    the note the stroke started with, and Shift+Draw keeps the row the drag started in while Paint
    follows the pointer into other rows. Both are one `note.add`, one undo step, and both select
    the notes they added.
  - **Paint**: pressing and dragging paints notes one note length apart, each in the row under the
    pointer at that moment, from where the press was in both directions; a slot already painted
    in the stroke, or holding a note of the same key at the same start, is skipped. The stroke is
    one `note.add`, one undo step, and the painted notes are selected. A click adds one note.
  - **Erase**: a click on a note deletes it; dragging deletes every note the pointer passes over
    (they fade while the drag lasts), one `note.delete`, one undo step.
  - **Slice**: pressing and dragging draws a cut line across the grid; when it is let go, every
    note the line crosses is split where the line crosses it, the cut on the grid (Alt: where the
    line is). A cut that lands on a note's start or end leaves that note alone; a vertical drag
    that crosses no note does nothing. The whole stroke is one `note.split`, one undo step, and
    the notes the cut made are selected. A click (no drag) does nothing.
  - **Mute** (built,
    [#460](https://github.com/suhina-daw/suhina-daw/issues/460)): a click on a note mutes it, or
    unmutes it when it is already muted; pressing and dragging sets every note the pointer passes
    over to the state the first note the stroke touched takes, so a stroke that starts on a muted
    note unmutes the run. The stroke is one `note.setMuted`, one undo step (› Muting). A click or
    drag on an empty spot does nothing. The tool works on the note under the
    pointer whatever the selection is, and never changes the selection, so a run can be muted
    without losing what was selected.
  - **B** (`pianoRoll.draw`, after a click in the piano roll) switches between Select and Draw. The
    number keys **1** to **6** (`pianoRoll.tool1` … `pianoRoll.tool6`) choose Select, Draw, Paint,
    Erase, Slice and Mute, read only while the piano roll has the keyboard, so the number keys keep
    their meaning elsewhere in the window.
  - A new note takes the length and velocity of the note last drawn or clicked (one grid step and
    100 until then), so repeated drawing keeps one feel. The cursor shows the tool (› Layout): a
    crosshair to draw or paint, an eraser to erase, a text cursor to slice, an arrow for Select and
    Mute.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** dragging up or down while
  drawing sets the length.

## Double-click

**Built ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)).** A double-click is two
presses of the left button in the piano roll, the second within 0.3 s of the first and within 6
points of it. The piano roll counts them itself, from its own clicks: a click anywhere else just
before (selecting the clip, a button) never turns the pair into something else, and a third quick
click starts a new pair rather than being read as a triple click. The first click of the pair
always does what a single click does; the table says what the second one adds.

| Where | Tool | The second click |
|---|---|---|
| Empty spot of the grid | **Select** | adds a note there (› Drawing notes) when **Double-click on empty grid** is **Adds a note** (every preset); with **Does nothing** it clears the selection, as a click does. When **Click on empty grid** is **Draws a note** (Mouse-first) the first click has already added the note, so the second one only selects it and never deletes it |
| Empty spot of the grid | **Draw**, **Paint** | selects the note the first click added: one note, one undo step |
| Empty spot of the grid | **Erase**, **Slice**, **Mute** | nothing |
| A note, or its right edge | **Select** | deletes the note when **Double-click on a note** is **Deletes it** (every preset), else selects it alone, as a click does. A note the first click of the pair added is never deleted |
| A note, or its right edge | **Draw**, **Paint**, **Slice** | selects it, as a click does |
| A note, or its right edge | **Erase** | nothing: the first click deleted the note, and the second does not delete the note under it, so a double-click deletes one note |
| A note, or its right edge | **Mute** | nothing: the first click muted (or unmuted) the note, and the second does not flip it back, so a double-click mutes a note in one undo step |
| Velocity lane | any | selects the stem's note, as a click does; nothing else |
| Key column | any | plays the key again, as a click does; nothing is added or selected |

A double-click is never a drag: a pair whose second press moves further than egui's click distance
is a drag and does what the tool's drag does. Every edit a double-click makes is one command and
one undo step, so Ctrl+Z (Cmd+Z on macOS) takes back exactly what it did.

## Playing the keys

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** pressing a key in the key
  column plays that note through the track's instrument for as long as the button is held: the note
  goes on at the press, off at the release, or when the pointer leaves the column. Dragging up and
  down the column plays the keys it passes over, one at a time: the key the pointer was on goes off
  and the new one goes on. The key under the pointer and the key that is held are drawn lit (a
  lighter row). Nothing is added to the project or to the undo history; a press that plays no note
  (no audio engine attached) changes nothing else either.
- The sound is a **transport**-kind command, `preview.noteOn` and `preview.noteOff` (› Commands):
  it goes to the engine the window is attached to (ADR 0011), the same path the browser's preview
  takes, and it never changes the project. The UI plays nothing when there is no engine.
- **Ctrl+click** on a key selects every note of that pitch in the clip; **Ctrl+drag** over keys
  selects the notes of every pitch the drag passes over, if any, and **Shift** adds them to the
  selection instead of replacing it. These change only the window's selection (› Selecting); the
  keys do not sound then, so a selection can be made without playing.

## Selecting

- **Built:** a click on a note selects it alone; **Shift**+click (or Ctrl+click) adds it to the
  selection or takes it out. A click on an empty spot of the grid clears the selection. Dragging
  from an empty spot draws a rectangle; when it is let go, the notes it touches are selected
  (with Shift, added to the selection).
- **Built:** the selection lives in the window only; it is not part of the project and is not an
  undo step. It is cleared when another clip is shown.
- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** while the piano roll has
  the keyboard (after a click in it), **Ctrl+A** selects every note of the clip and **Escape** clears
  the selection. Elsewhere Escape keeps its other uses.
- **Later:** a click on the ruler selects a time range, and every note starting in it; a lasso.
  Ctrl+click and Ctrl+drag on the key column, which select notes of a pitch or a range of
  pitches, are built (› Playing the keys).

## Moving

- **Built:** dragging a selected note (or any note, which selects it first unless it is already in
  the selection) moves the whole selection: sideways in time, snapped by whole grid steps (Alt:
  freely), up and down by semitones. The notes are shown where they go while dragging; the move is
  one command, one undo step, when the mouse is let go. A move that would put a note before the
  content's start or outside keys 0 to 127 stops at that limit, for the whole selection, so the
  selection keeps its shape.
- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** while the piano roll has
  the keyboard, **Left** and **Right** move the selection one grid step earlier or later (the grid
  as drawn; a beat with `none`), **Up** and **Down** a semitone, **Shift+Up** and **Shift+Down** an
  octave. Each press is one `note.move`, one undo step. A step in time that would cross the
  content's start stops at it; a pitch move that does not fit for every selected note does nothing,
  so no note changes its name.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** Ctrl+drag copies the notes
  instead of moving them. Whether moving notes past the clip's end extends the clip is
  **(to decide)**.

## Resizing

- **Built:** the pointer over the last few pixels of a note shows a resize cursor; dragging there
  changes the length of every selected note by the same amount, the dragged note's end snapping to
  the grid (Alt: freely). Dragged onto or before its own start, the dragged note's end goes to the
  first grid line after its start; no selected note gets shorter than a hundredth of a beat.
- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** **Shift+Right** and
  **Shift+Left** make every selected note one grid step longer or shorter (`note.resize` with `by`).
  The keys never make a note shorter than one step: Shift+Left shortens by less, or not at all,
  when the shortest selected note would get shorter than that.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** dragging a note's left edge
  moves its start and keeps its end; a modifier while resizing stretches the selection in time
  around its start (scale the positions as well as the lengths).

## Deleting

- **Built:** **Delete** or **Backspace** while the piano roll has the keyboard (after a click in
  it) deletes the selected notes, one undo step. Without a note selected, those keys keep doing
  what they do elsewhere (delete the selected clip). A double-click on a note deletes it.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** a drag with the right
  button deletes notes. A **right-click** on a note deletes it in the Draw and Paint tools when
  the **Draw-first** keymap preset is chosen (› Keymap presets), and in every tool when the
  **Mouse-first** preset's **Right click deletes a note** switch is on; otherwise it does nothing.

## Muting

- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** a muted note stays in
  the clip and does not play: the engine emits no voice for it, exactly as if the note were not
  there, through the arrangement, the launcher and an offline render. It keeps its time, length,
  key and velocity.
- The **Mute** tool (key **6**) mutes and unmutes the note under the pointer (› Drawing notes); a
  click toggles one note, and a drag sets every note it passes over to the state the first note the
  stroke touched takes, so a run is muted in one go without each note flipping on its own. The drag
  is one stroke, so it is one `note.setMuted`, one undo step: Ctrl+Z (Cmd+Z on macOS) puts the whole
  stroke back at once.
- A muted note is drawn as the note it is, with a faded body, a dashed edge and no velocity stem
  in the lane standing out (its stem is drawn faint), so it is clearly still there and clearly not
  playing. The note's name and its position on the grid are unchanged.
- Mute is part of the project ([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment:
  muted notes): saving writes `"muted": true` on those notes only, so a project without muted
  notes is written as before, and reading a file from before this reads its notes as unmuted. A
  muted note stays muted after the project is saved and opened again, and a Live Set's note marked
  `IsEnabled="false"` is read as a muted note instead of being dropped
  ([.als format](als-format.md) › Notes).
- Copying, cutting, pasting and duplicating a note carries its mute with it; `note.setMuted` is the
  only command that changes it. The MIDI tools that change notes (transpose, reverse and the rest,
  › Other note tools) keep each note's mute, and act on muted notes as they do on the others, so a
  part can be muted, moved and unmuted without the mute being lost.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** per-note probability and
  velocity range; muting a whole clip or a time range of the grid.

## Copy, cut, paste and duplicate

- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** while the piano roll has
  the keyboard, **Ctrl+C** copies the selected notes (`note.copy`), **Ctrl+X** cuts them
  (`note.cut`: copies and deletes, one undo step), **Ctrl+V** pastes the copied notes
  (`note.paste`) and the duplicate key duplicates the selected notes (`note.duplicate`). The
  duplicate key is **Ctrl+B** with the **Draw-first** keymap preset and **Ctrl+D** with
  **Select-first**, the default (› Keymap presets); it can be changed like any other key.
  With no note
  selected (or, for Ctrl+V, no notes copied) the keys act on the selected clip, as elsewhere.
- The keys are Ctrl on Windows and Linux and Cmd on macOS. The window reports them (and the other
  system keys for the same thing: Shift+Delete, Ctrl+Insert and Shift+Insert on Windows, and the
  Cut, Copy and Paste keys some keyboards have) as cut, copy and paste rather than as keys; the
  keymap reads them as Ctrl+X, Ctrl+C and Ctrl+V. The window reports a paste only while the
  system clipboard holds text, so copying or cutting notes or a clip in Suhina also puts a short
  line ("Suhina: notes copied") on the system clipboard.
- The toolbar's Cut, Copy, Paste and Duplicate icon buttons (› Layout) run these same commands on
  the selected notes, greyed out when there are none; Ctrl+X and Ctrl+C keep working whichever
  preset is chosen.
- Copied notes keep their spacing, their keys, lengths and velocities. Paste puts the first of them
  right after the end of the selected notes; with none selected, at the playhead when it is inside
  the clip (a looping clip maps it into its loop); else right after where they were copied from.
  Duplicate puts the copy right after the selection's end (the selection spans from its earliest
  start to its latest end). Pasted and duplicated notes are selected; the notes they land on stay.
- The note clipboard belongs to the open project's session, apart from the clip clipboard; notes
  copied in one MIDI clip paste into any other. It is not saved with the project.

## Velocity

- **Built:** in the velocity lane, a note's bar is grabbed anywhere over the note's width (and a
  few pixels before its start, where its stem is drawn); dragging it sets the note's velocity to
  the height of the pointer in the lane (the top is 127, the bottom 1). With several notes
  selected and the dragged one among them, the others change by the same amount, each kept within
  1 to 127. The lane shows the values while dragging; letting go is one `note.setVelocity`, one
  undo step.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** a note's velocity changed by dragging on the note itself with a modifier (Alt+drag up
  and down); drawing a line across the lane to set many stems at once; release velocity and a
  note's probability and velocity range (a per-note chance to play and a random variation), shown
  as a second lane; randomise, ramp and deviation tools.

## Quantise

- **Built ([#345](https://github.com/suhina-daw/suhina-daw/issues/345), in the clip column
  [#443](https://github.com/suhina-daw/suhina-daw/issues/443), moved to the MIDI tools column
  [#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** Ctrl+U quantises the
  selected notes, or every note of the clip when none is selected, with the last settings, at
  once. The quantise controls are the **Quantise** block at the top of the MIDI tools column
  (› Other note tools), on the left of the note grid: **Grid** (1/32 to 8 bars,
  triplets too; 1/16 at first), **Amount** (0 to 100 %, 100 at first) and **Move ends too** (off
  at first), and the buttons **Quantise notes** (quantises now) and **Close**. The clip's own
  properties (Track, Start, Length, Loop, Notes) stay in the properties column beside it. Nothing
  floats over the note grid.
- Escape and **Close** close the block, and the properties move back up. A press outside it does
  not close it (the block is part of the column, not a pop-up). The block's settings are kept for
  Ctrl+U until Suhina closes.
- Each start moves towards the nearest grid line by the amount: 100 % lands on the line, 50 %
  goes half way, 0 % changes nothing. A note already on a line keeps its exact start. A tie
  between two lines goes to the later one. Without ends a note keeps its length; with ends its
  end moves towards the nearest line the same way, and when that line is on or before the
  quantised start, to the first line after it, so a note never becomes zero long.
- The block's grid is its own and does not follow the piano roll's grid value (nor the control
  bar's), so snapping while drawing and quantising can differ. Bars are as long as the project's time signature
  makes them.
- The edit is `note.quantize` (see Commands), one undo step, so it also runs from the CLI and MCP.

### Clip column

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** the clip properties
  column is the same column in every layout: Track, Start, Length, Loop and (for a MIDI clip)
  Notes, or the audio clip's Gain and fades. With the detail area stacked (a column at the right of
  the window) it is a block at the top and the piano roll sits under it, as before.
- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** the **Notes** row reads
  **"N of M"** while notes are selected, where N is how many are selected and M how many the clip
  has, and just **M** with nothing selected. Under it, while a selection exists, come the rows that
  describe it, in this order and only when they say something:
  - **From** and **To**: the selection's span in the clip's beats, as the ruler writes them (bar
    and beat, such as `2.1` and `2.3`), from the earliest start to the latest end. They describe
    the notes as they are on the grid, so a drag shows the values it would land on only when it is
    let go.
  - **Pitch**: the range of the selection's keys, by name, such as `C3` for one key or `C3–G3` for
    several (the lowest and the highest, in that order).
  - **Velocity**: the range of the selection's velocities as 1 to 127, such as `64` or `64–127`.
  - A selection of muted notes says how many are muted in the Notes row, such as `2 of 4, 1 muted`,
    so a run muted with the Mute tool is visible in the column as well as on the grid.
- **Built:** the column never wraps a value onto more lines, at any height or width. A label or a
  value too long for its row is shortened with an ellipsis; rows that do not fit under the
  column's bottom are left out, from the last one up, rather than drawn cut in half, and the title
  goes last. The same holds for the MIDI tools column beside it: its buttons and fields shorten
  their text instead of wrapping it, and the column is left out when its head does not fit.
- When the quantise block is open it takes the column's top: its title **Quantise**, then Grid,
  Amount, the checkbox and the two buttons, and the clip's own properties are drawn under it. The
  space the block needs is given to the column for that frame; closing the block gives it back.
- The selection rows are window state, like the selection itself: not part of the project, not an
  undo step, and gone when the selection is cleared or another clip is shown (› Selecting).
- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** the MIDI tools are their
  own column beside this one (› Other note tools), drawn between the properties and the editor; a
  later detail-area block takes its place in this column's list of blocks rather than changing its
  shape.
- **Later:** swing; quantise while recording (record quantisation) belongs to recording, which has
  its own issue.

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
  default **(to decide)**.

## Slicing and gluing

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** the **Slice** tool (›
  Drawing notes) splits the notes a drawn cut line crosses, and `note.split` (see Commands) does
  the same for named notes at one beat, so the CLI and MCP can split too. One stroke is one
  command, one undo step.
- **Later ([#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** a command that chops the
  selection into notes of one grid step each; glue (join) touching notes of the same key into one
  note (Ctrl+G); legato (each note lasts until the next one starts); the command `note.join`.

## Chords

- **Later ([#350](https://github.com/suhina-daw/suhina-daw/issues/350)):** a chord tool: draw one note and a chosen chord (major, minor, seventh and so on, or
  one from the clip's scale) is added on top of it. Strum and arpeggiate a selected chord: spread
  the starts of its notes over a time set by dragging (strum), or play them one after another on
  the grid (arpeggiate). Inversions move the lowest or highest note an octave.

## Other note tools (MIDI tools column)

- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460), the column itself
  [#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** the editor has a **MIDI tools
  column** between the clip properties column and the note grid, shown while a MIDI clip is open
  and hidden by a click on the **MIDI tools** icon in the toolbar (› Layout). Showing or hiding it
  is window state: it is not a command, not saved with the project and not an undo step, and
  hiding it never changes the notes or the selection. The column is narrower than the properties
  column and scrolls on its own when the detail area is short.
- Its head says what the tools act on: **Selected notes** while notes are selected, otherwise
  **Whole clip**, with the count under it (`4 notes` or `12 of 12 notes`). The tools with a
  setting of their own (Transpose's semitones, Humanise's **Time** and **Velocity**, the scale's
  root and mode, the velocity tools) keep what was last typed until Suhina closes, as the quantise
  block's settings do.
- Each tool is a button that runs one command over the notes the head names, one undo step, so the
  same edit runs from the CLI and MCP (› Commands):

  | Tool | Does | Command |
  |---|---|---|
  | **Quantise** | Moves the starts (with **Move ends too**, the ends) towards the grid by **Amount** | `note.quantize` |
  | **Transpose** | Moves the notes by the semitones in its field, −24 to +24 | `note.move` with `semitones` |
  | **Humanise** | Moves each start by up to **Time** beats and each velocity by up to **Velocity** steps, a fresh draw each time, so pressing it twice gives two different results | `note.humanize` |
  | **Fit to scale** | Moves each note to the nearest key of the chosen root and mode | `note.fitToScale` |
  | **Invert** | Mirrors the pitches around the middle of the selection's range | `note.invert` |
  | **Reverse** | Turns the selection round in time, inside its own span | `note.reverse` |
  | **Legato** | Makes each note last until the next one starts, at most | `note.legato` |
  | **×2**, **÷2** | Stretches or squeezes the selection in time, its starts and its lengths | `note.stretch` |
  | **Velocity: Randomise** | Changes every velocity by a random amount up to **Amount** | `note.randomizeVelocity` |
  | **Velocity: Ramp** | Draws the velocities from **From** to **To** across the notes in time | `note.rampVelocity` |

- The quantise controls live here, at the top of the column with the rest: the **Quantise** block
  of the clip column (Grid, Amount, **Move ends too**, the **Quantise notes** button and **Close**)
  moved into this column, and Ctrl+U keeps running the same command with the same settings while
  the column is hidden. With the detail area stacked (a column at the right of the window) the
  MIDI tools column has no room and is not drawn, whatever the toolbar icon says; Ctrl+U and the
  keymap's tools still work there.
- **Invert** mirrors each note's key around the middle of the range the tool acts on: the lowest
  and highest key named (the selection's, or the clip's), so `C3` and `C5` swap and `C4` stays.
  **Reverse** puts the last note first inside the span from the earliest start to the latest end,
  each note keeping its length, so a rising line becomes a falling one at the same place in the
  clip. **×2** and **÷2** stretch relative to the earliest start, keeping the first note where it
  is. **Legato** moves each note's end to the next note's start (never making a note longer than
  the gap to the next, never shorter than a hundredth of a beat), in time order per key, so a chord
  stays together as far as the next note of its own key.
- **Later ([#350](https://github.com/suhina-daw/suhina-daw/issues/350)):** the chord tool, strum
  and arpeggiate, inversions; a length field (`note.resize`) and a legato mode that also moves
  starts.

## Shortcuts

Built (the first version, and keyboard editing from [#344](https://github.com/suhina-daw/suhina-daw/issues/344)):

| Keys | Does |
|---|---|
| Double-click on an empty spot | Adds a note (Select tool; › Double-click) |
| Double-click on a note | Deletes it (Select tool; › Double-click) |
| Click, Shift+click, Ctrl+click | Selects a note, adds or takes it out of the selection |
| Drag from an empty spot | Selects the notes in the rectangle |
| Drag a note | Moves the selection (time snapped by grid steps, pitch by semitones) |
| Drag a note's right edge | Changes the selection's length |
| Alt while drawing or dragging | No snapping |
| Ctrl+1, Ctrl+2 | Makes the grid finer, coarser (Narrow grid, Widen grid; after a click in the piano roll) |
| Ctrl+3 | Turns the triplet grid on or off (after a click in the piano roll) |
| Ctrl+4 | Turns snapping to the grid on or off (after a click in the piano roll) |
| Drag a bar in the velocity lane | Sets its note's velocity to the pointer's height (the selection by the same amount) |
| Delete, Backspace | Deletes the selected notes (after a click in the piano roll) |
| Ctrl+U | Quantises the selected notes, or every note, with the last settings |
| Ctrl+A, Escape | Selects every note, clears the selection |
| Left, Right | Moves the selection one grid step earlier, later |
| Up, Down; Shift+Up, Shift+Down | Moves the selection a semitone; an octave |
| Shift+Left, Shift+Right | Makes the selection a grid step shorter, longer |
| Ctrl+C, Ctrl+X, Ctrl+V | Copies, cuts, pastes notes |
| Ctrl+D, or Ctrl+B with Draw-first | Duplicates the selected notes (› Keymap presets) |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo (as everywhere) |
| Wheel, Shift+wheel | Scroll keys, scroll time |
| Ctrl+wheel; Ctrl+Shift+wheel or Alt+wheel | Zoom time; zoom key height |
| Right button held + wheel | Chooses the next or previous tool, stopping at the first and last |
| Shift+drag with Draw | Draws notes back to back, the last note's length each |
| 1, 2, 3, 4, 5, 6 | Chooses Select, Draw, Paint, Erase, Slice, Mute (after a click in the piano roll) |
| B | Switches between the Select and Draw tools (after a click in the piano roll) |
| Click, drag in the key column | Plays the key, through the track's instrument |
| Ctrl+click, Ctrl+drag in the key column | Selects the notes of that pitch, of those pitches |
| Middle-button drag | Pan |
| Ctrl+Shift+E | Hides the editor (the detail area), or shows it again (`view.editor`, › Least height and closing) |

The keys after a click in the piano roll are keymap actions ([`docs/shortcuts.md`](../shortcuts.md)):
the `selection.*` ones the window has anyway, and `pianoRoll.selectAll`, `.deselect`, `.earlier`,
`.later`, `.up`, `.down`, `.octaveUp`, `.octaveDown`, `.shorter`, `.longer`, `.draw`,
`.tool1` … `.tool6`, `.gridNarrow`, `.gridWiden`, `.gridTriplet` and `.gridSnap`, which are read only
while the piano roll has the keyboard, so they can be changed like the rest. `pianoRoll.quantize`
(Ctrl+U) means nothing elsewhere, so it works whenever the piano roll shows a clip.

Later: Ctrl+G (glue), Z (zoom to selection), added to the keymap when built.

## Workflow modes

**Built ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)).** The way the piano roll
works is a **mode**: one of the three presets below, or **Custom**. The **workflow menu** in the
piano roll's toolbar, after the MIDI tools icon, shows the mode's name (its accessible label is
"Workflow" and the name, such as "Workflow Select-first") and can be changed at any time, while
editing; Settings › Shortcuts › **Preset** offers the same four modes. The menu lists the four
modes, ticked as chosen, and under a divider every **workflow setting** with its choices:

| Setting | Choices | Select-first | Draw-first | Mouse-first |
|---|---|---|---|---|
| **Click on empty grid** (Select tool) | Clears the selection, Draws a note | Clears | Clears | Draws |
| **Right click on a note** | Does nothing, Deletes in Draw and Paint, Deletes in every tool | Nothing | Draw and Paint | Every tool |
| **Shift+right drag** | Does nothing, Splits notes | Nothing | Nothing | Splits |
| **Alt+right button** | Does nothing, Plays from the point | Nothing | Nothing | Plays |
| **Duplicate key** | Ctrl+D, Ctrl+B | Ctrl+D | Ctrl+B | Ctrl+D |
| **Double-click on empty grid** (Select tool) | Adds a note, Does nothing | Adds | Adds | Adds |
| **Double-click on a note** (Select tool) | Deletes it, Does nothing | Deletes | Deletes | Deletes |

- Choosing a preset fills in every setting with its column. Changing one setting, in the menu or
  in Settings › General › Piano roll (one row per setting), makes the mode **Custom**, with the
  other settings kept as they were. Choosing **Custom** itself changes no setting, so a preset's
  column can be the start of a mix. A Custom mix that happens to equal a preset's column stays
  Custom until the preset is chosen.
- **Duplicate key** binds the key: Ctrl+B moves the browser's key to Ctrl+Shift+B, Ctrl+D gives
  the browser Ctrl+B back. The key can still be changed in the Shortcuts list; the setting then
  reads Ctrl+B only while the duplicate action is bound to Ctrl+B.
- The mode and every setting are kept in `keymap.json` with the keys: `"preset"` names the mode
  (`"Custom"` included) and each setting has its own entry, `"workflowEmptyClick"` (`"selects"`,
  `"draws"`), `"workflowRightClick"` (`"nothing"`, `"drawAndPaint"`, `"everyTool"`),
  `"workflowShiftRight"` and `"workflowAltRight"` (`"nothing"`, `"on"`), `"workflowDuplicate"`
  (`"Ctrl+D"`, `"Ctrl+B"`), `"workflowDoubleClickEmpty"` (`"adds"`, `"nothing"`) and
  `"workflowDoubleClickNote"` (`"deletes"`, `"nothing"`). The four `"mouse*"` entries of earlier
  files are still written and read (› Keymap presets below). A setting the file leaves out is the
  preset's; a value that is not one of its choices is ignored with a warning in the status bar. A
  file whose settings are not the ones its preset fills in (an earlier file with a mouse switch
  turned off by hand) opens as **Custom** with those settings.
- Changing the mode or a setting is window state, saved with the keymap: it is not a command, not
  part of the project and not an undo step, and it never changes the notes or the selection.

## Keymap presets

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443), Mouse-first
  [#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** Settings › Shortcuts has
  a **Preset** menu with three choices, which set the keys of a few actions and the mouse's
  behaviour at once:

  | Preset | What it sets | Where it differs |
  |---|---|---|
  | **Select-first** (default) | the keys Suhina has today | duplicate is **Ctrl+D**; a right-click on a note does nothing |
  | **Draw-first** | the same, with the keys of someone who draws notes first | duplicate is **Ctrl+B**; in the Draw and Paint tools a right-click on a note deletes it |
  | **Mouse-first** | the clicks of someone who works with the mouse: every switch below on, and the left button draws in the **Select** tool | duplicate is **Ctrl+D**; the four switches are on |

- **Mouse-first gives the mouse the four jobs a pen and modifier keys would do**, each a switch of
  its own under Settings › General › Piano roll, so any of them can be turned off and the rest
  kept:
  - **Left click draws** (on by default with Mouse-first): in the **Select** tool, a press on an
    empty spot of the grid draws a note there and dragging sets its length, as the Draw tool does,
    and a press on a note moves it, as Select does. Off, the left button selects as Select-first
    does.
  - **Right click deletes a note** (on): a right-click on a note deletes it; dragging with the
    right button held deletes every note the pointer passes over, one `note.delete`, one undo
    step, in every tool.
  - **Shift+right click splits** (on): a right-click with Shift held slices the notes under a
    short drag the way the Slice tool does, one `note.split`, one undo step.
  - **Alt+right plays from the point** (on): while the right button is held with **Alt** (Option
    on macOS), the clip plays from the spot under the pointer, and it stops when the button is let
    go. It is the transport preview of › Playing the keys, for the whole clip: nothing is added to
    the project and it is not an undo step. Held with the wheel, the right button still chooses the
    tool (› Zoom and scroll).
- A switch turned on or off by hand is a workflow setting changed by hand, so the mode becomes
  **Custom** (› Workflow modes); choosing **Mouse-first** again turns them all back on. The switches are kept with the keymap in
  `keymap.json`, under `"mouseLeftDraws"`, `"mouseRightDeletes"`, `"mouseShiftRightSplits"` and
  `"mouseAltRightPlays"`, each `true` or `false`; a file without them takes Select-first's (all
  off), and a value that is not a boolean is ignored with a warning, as an unknown key is.

- Choosing a preset sets the defaults of the keys it names; every other key keeps what it has, and
  single keys can still be changed afterwards, as today (› Settings in
  [`docs/shortcuts.md`](../shortcuts.md)). Choosing a preset twice sets them again, so a key that
  was changed by hand comes back to the preset's.
- The choice is saved with the keymap in `keymap.json` under `"preset"`, and read back when the
  file is read. A file without it is **Select-first**; **Custom** is read and written too. A `"preset"` naming something else is
  ignored: the keymap is Select-first and the window says so in the status bar, so a hand-edited
  file never leaves Suhina with no keys.
- The preset names describe the habit they suit; they are not the name of anything else.

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
| `note.quantize` | clip, `notes?`, `grid`, `amount?`, `ends?` | Moves the starts (with `ends: true` the ends too) of the notes, or of every note when `notes` is left out, towards the nearest line of `grid` (as `launcher.setQuantization` takes it, such as `"1/16"` or `"1/8T"`; not `none`) by `amount` percent, 0 to 100 (default 100); returns the indices and how many moved |
| `note.setMuted` | clip, `notes`, `muted?` | Mutes them (`muted: true`) or unmutes them (`false`); with `muted` left out each note is flipped. Returns the indices and their state |
| `note.humanize` | clip, `notes?`, `time?`, `velocity?`, `seed?` | Moves each note's start by up to `time` beats (0 to 1, default 0.02) and its velocity by up to `velocity` MIDI steps (0 to 127, default 8), each a fresh random draw; `seed` makes the same call repeatable; returns the indices |
| `note.fitToScale` | clip, `notes?`, `root?`, `scale?` | Moves each note to the nearest key of `scale` (`major`, `minor`, `harmonicMinor`, `melodicMinor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`, `majorPentatonic`, `minorPentatonic`, `blues`; default `major`) over `root` (a key name such as `"C"`, `"F#"`, or 0 to 11; default `"C"`); a tie goes to the lower key; returns the indices and how many moved |
| `note.invert` | clip, `notes?` | Mirrors the keys around the middle of the range the notes cover (the lowest key plus the highest, halved, rounded down); returns the indices |
| `note.reverse` | clip, `notes?` | Turns the notes round in time inside their own span (the earliest start to the latest end): each note's start becomes `span end − its end`, its length unchanged; returns the indices |
| `note.legato` | clip, `notes?` | Makes each note last until the next note **of its own key** starts, or leaves it when nothing follows; never shorter than a hundredth of a beat; returns the indices and how many changed |
| `note.stretch` | clip, `notes?`, `factor` | Stretches the notes in time about the earliest of their starts (`0.01` to `100`): starts and lengths are multiplied, the first start unchanged. `2` is the **×2** button, `0.5` the **÷2** one; returns the indices |
| `note.randomizeVelocity` | clip, `notes?`, `amount?`, `seed?` | Moves each velocity by up to `amount` MIDI steps (0 to 127, default 16), each kept within 1 to 127; `seed` makes the same call repeatable; returns the indices |
| `note.rampVelocity` | clip, `notes?`, `from`, `to` | Sets the velocities from `from` to `to` (1 to 127) across the notes in time order, evenly spaced; one note takes `to`; returns the indices |
| `note.copy` | clip, `notes` | Copies them to the note clipboard; changes nothing and is no undo step |
| `note.cut` | clip, `notes` | Copies them and deletes them |
| `note.paste` | clip, `time?` | Adds the copied notes, the first at `time` (default: right after the copied notes); returns their indices |
| `note.duplicate` | clip, `notes` | Adds a copy right after the notes' end; returns its indices |
| `note.split` | clip, `notes`, `at` | Splits each named note that spans the beat `at` into two at `at`; a note that starts or ends at `at` is left alone; returns the indices the splits made |
| `preview.noteOn` | `track`, `key`, `velocity?` | Plays `key` through the track's instrument until `preview.noteOff` (transport kind: the project and the undo history are untouched) |
| `preview.noteOff` | `track`, `key?` | Releases the preview note of `key` on that track, or every held preview note when `key` is left out |

Limits, so a hostile input cannot break the project or exhaust memory: time from 0 to 120 000 beats
(the engine's furthest start), length above 0 and at most 120 000 beats, keys 0 to 127, velocity 1
to 127 for a set value, at most 10 000 notes named or added in one command and 100 000 notes in a
clip. A move or resize that would take any note out of these limits fails as a whole; the piano
roll limits its drags beforehand so a drag never fails. A list of indices that repeats one counts
it once. A split names at most 10 000 notes and adds at most one note per named note, so it can
never grow a clip past the 100 000 notes it holds; `at` is a beat from 0 to 120 000.

## Open

- **Alt+wheel over a note:** today it zooms the key rows. Proposed: over a note it changes that
  note's velocity (every selected note when the note under the pointer is selected) and over the
  empty grid it keeps zooming the rows, as the Arrangement view's clip gain does. Not built: the
  maintainer decides ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)).
- **The toolbar size default: Large (125 %), as built.** Normal (100 %) was considered, and the
  maintainer decides ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)); changing it is
  one constant and the default in Settings, and an existing `settings.json` keeps the value it
  stored.
- **Mute is the sixth tool**, after Slice, with the number key **6**. Another order (Mute before
  Slice) changes the labels of `pianoRoll.tool5` and `pianoRoll.tool6` and the icons; the
  maintainer decides ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)).
- **Double-click on a note in Select deletes it** in every preset, as built before; making
  **Does nothing** the default for Mouse-first (whose right button deletes) is the maintainer's
  call ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)).
- **The preset names** (**Select-first**, **Draw-first** and **Mouse-first**) describe the habit;
  the maintainer may
  rename them ([#443](https://github.com/suhina-daw/suhina-daw/issues/443),
  [#460](https://github.com/suhina-daw/suhina-daw/issues/460)).
- Envelopes and Expression tabs (per-note pitch bend, pressure, timbre; clip automation): a
  separate spec.
- Audio clip editor (sample view, warp markers): a separate spec.
- MIDI recording into a clip, overdub and step input: belongs to recording.
