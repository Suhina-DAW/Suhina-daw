---
description: Functional spec of the MIDI clip editor (piano roll): layout, toolbar icons and tools, grid and snapping, zoom, drawing, selecting, moving, resizing, velocity, quantise, scales, ghost notes, slicing, a playable key column, chords, shortcuts and keymap presets, and the note.* and preview.* commands
read_when: piano roll, clip editor, toolbar, toolbar icons, tool icons, draw tool, paint tool, erase tool, slice tool, B key, number keys choose a tool, right button and wheel, Shift and Draw, pulled to the top, maximise piano roll, piano roll grid value, MIDI editor, note editing, draw notes, select notes, move notes, resize notes, velocity lane, velocity, grid, snapping, Alt off-grid, zoom, detail area height, scale highlighting, fold, ghost notes, quantise, quantize, inline quantise, clip column, slice, glue, chords, strum, playable keyboard, key column plays, preview.noteOn, preview.noteOff, note.add, note.delete, note.move, note.resize, note.setVelocity, note.split, note.list, note.quantize, Ctrl+U, note.copy, note.cut, note.paste, note.duplicate, copy paste notes, arrow keys, select all notes, keymap preset, Draw-first, Select-first, duplicate key
type: spec
status: draft
last_verified: 2026-10-15
covers:
  - crates/ui/src/piano_roll.rs
  - crates/session/src/notes.rs
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

From left to right: the clip properties column (exists), the MIDI tools column (later), the editor.
The editor, top to bottom:

1. **Toolbar** (built, [#442](https://github.com/suhina-daw/suhina-daw/issues/442), icons
   [#443](https://github.com/suhina-daw/suhina-daw/issues/443)): the tools, then the note
   clipboard, and at the right end the piano roll's grid value (› Grid and snapping). **Later:**
   tabs for Notes, Envelopes and Expression; Fold; Scale with the scale name.

   The tools are **Select**, **Draw**, **Paint**, **Erase** and **Slice** (› Drawing notes), each an
   icon button: the shape is drawn in code with our own strokes ([`crates/ui/src/browser_view.rs`](../../crates/ui/src/browser_view.rs)
   › `icon` does the same for the browser), on a 14 px box, 1.1 px wide, in `text.secondary`; the
   chosen tool's is `text.primary` on `control.activeBackground`. A click chooses the tool; the
   number keys **1** to **5** do too, after a click in the piano roll (› Shortcuts). The pointer
   over a button shows its name and its current key, such as "Draw (2)", and the button has that
   name as its accessible label, so a reader and a test find it by name.

   After a divider, **Cut**, **Copy**, **Paste** and **Duplicate** icon buttons run what
   Ctrl+X, Ctrl+C, Ctrl+V and the duplicate key run (› Copy, cut, paste and duplicate) on the
   selected notes; they are greyed out (a `text.hint` shape, no highlight, no action) when there
   is nothing to act on: no note selected for Cut, Copy and Duplicate, nothing copied for Paste.
   The grid value stays at the right end of the toolbar; the quantise controls are in the clip's
   properties column (› Quantise).
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
   detail area: at most 30 % of its height). One stem per note at the
   note's start, its height the velocity; a selected stem is brighter. **Later:** the lane's
   height dragged by its top edge; chord notes on the same start drawn side by side, a few pixels
   apart, so each can be grabbed; the lane's tools (randomise, ramp, deviation) ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)).

Everything scales with the detail area: the detail area's splitter (exists) makes the editor taller
or shorter, and the key rows and the velocity lane keep their own heights while the visible range
of keys grows and shrinks.

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

- **Built ([#442](https://github.com/suhina-daw/suhina-daw/issues/442)):** the piano roll has its own
  grid value, the menu at the right end of its toolbar (none, 8 bars to 1 bar, 1/2 to 1/32 and the
  triplets), 1/16 until another is picked; it lives in the window, not the project. The control
  bar's grid value (the launch quantisation, `launcher.setQuantization`) then applies to the
  Arrangement view only. Settings › General › Piano roll › **Piano roll grid** set to **Control
  bar** makes the piano roll use the control bar's value instead, and its menu then sets the
  control bar's. The same value draws the
  vertical lines and is the step that notes snap to. When lines would be closer than a few pixels,
  every second line is left out until they are far enough apart (snapping uses the same, coarser
  step, so what snaps is what is drawn). `none` draws beats and bars and does not snap.
- **Built:** starts and ends of drawn, moved and resized notes snap to the grid. Holding **Alt**
  while drawing or dragging places them freely (no snapping). A move keeps a note's distance to the
  grid: the drag snaps the **offset** to whole grid steps, so a note that was off the grid stays
  just as far off (it keeps a played-in groove).
- **Later ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)):** an adaptive grid that
  follows the zoom (finer when zoomed in), triplet and off toggles, keyboard shortcuts to make the
  grid finer or coarser (Ctrl+1 and Ctrl+2) and to toggle snapping (Ctrl+4) and triplets (Ctrl+3).
  Once the Arrangement view's grid (pull request #320) lands, the piano roll uses its line logic.
- **Later ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)):** "magnetic" snapping to other notes' starts and ends within a few pixels.

## Zoom and scroll

- **Built:** the mouse wheel scrolls up and down through keys; **Shift**+wheel scrolls in time;
  **Ctrl**+wheel zooms time around the pointer; **Ctrl+Shift**+wheel or **Alt**+wheel (Option on
  macOS, as in the Arrangement view) zooms key height around the pointer. Dragging with the middle mouse button pans both ways. A newly shown clip opens scrolled
  so its notes are in view (the middle of its lowest and highest note; C3 for an empty clip) and
  zoomed so the whole clip fits the width.
- **Built:** while the **right** mouse button is held down over the grid, each wheel step chooses
  the next tool, or with the wheel turned the other way the previous one, in the order Select →
  Draw → Paint → Erase → Slice, stopping at the first and the last tool (no wrapping round)
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
  selected. A double-click on a note deletes it.
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
  - **B** (`pianoRoll.draw`, after a click in the piano roll) switches between Select and Draw. The
    number keys **1** to **5** (`pianoRoll.tool1` … `pianoRoll.tool5`) choose Select, Draw, Paint,
    Erase and Slice, read only while the piano roll has the keyboard, so the number keys keep
    their meaning elsewhere in the window.
  - A new note takes the length and velocity of the note last drawn or clicked (one grid step and
    100 until then), so repeated drawing keeps one feel. The cursor shows the tool: a crosshair to
    draw or paint, a no-entry sign to erase, a text cursor to slice.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** dragging up or down while
  drawing sets the length.

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
  button deletes notes. A **right-click** on a note deletes it in the Draw and Paint tools when the
  **Draw-first** keymap preset is chosen (› Keymap presets); with **Select-first**, the default,
  it does nothing there.

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
  [#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** Ctrl+U quantises the
  selected notes, or every note of the clip when none is selected, with the last settings, at
  once. The quantise **icon button**, the first of the toolbar's second group, opens the quantise
  controls **in the clip's properties column** on the left of the detail area (› Clip column,
  below): the column takes a **Quantise** block at its top with **Grid** (1/32 to 8 bars,
  triplets too; 1/16 at first), **Amount** (0 to 100 %, 100 at first) and **Move ends too** (off
  at first), and the buttons **Quantise notes** (quantises now) and **Close**. The clip's own
  properties (Track, Start, Length, Loop, Notes) move down under the block. Nothing floats over the
  note grid.
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
- When the quantise block is open it takes the column's top: its title **Quantise**, then Grid,
  Amount, the checkbox and the two buttons, and the clip's own properties are drawn under it. The
  space the block needs is given to the column for that frame; closing the block gives it back.
- **Later:** the MIDI tools column ([#349](https://github.com/suhina-daw/suhina-daw/issues/349))
  joins these blocks in the same column; other detail-area work adds a block of its own rather than
  changing the column's shape.
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

- **Later ([#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** transpose by a number of semitones (a field), ×2 and ÷2 (stretch the selection or the
  clip in time), reverse, invert (mirror pitches), humanise (small random moves of start and
  velocity), set length. Each is a command over the selection, one undo step.

## Shortcuts

Built (the first version, and keyboard editing from [#344](https://github.com/suhina-daw/suhina-daw/issues/344)):

| Keys | Does |
|---|---|
| Double-click on an empty spot | Adds a note |
| Double-click on a note | Deletes it |
| Click, Shift+click, Ctrl+click | Selects a note, adds or takes it out of the selection |
| Drag from an empty spot | Selects the notes in the rectangle |
| Drag a note | Moves the selection (time snapped by grid steps, pitch by semitones) |
| Drag a note's right edge | Changes the selection's length |
| Alt while drawing or dragging | No snapping |
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
| 1, 2, 3, 4, 5 | Chooses Select, Draw, Paint, Erase, Slice (after a click in the piano roll) |
| B | Switches between the Select and Draw tools (after a click in the piano roll) |
| Click, drag in the key column | Plays the key, through the track's instrument |
| Ctrl+click, Ctrl+drag in the key column | Selects the notes of that pitch, of those pitches |
| Middle-button drag | Pan |

The keys after a click in the piano roll are keymap actions ([`docs/shortcuts.md`](../shortcuts.md)):
the `selection.*` ones the window has anyway, and `pianoRoll.selectAll`, `.deselect`, `.earlier`,
`.later`, `.up`, `.down`, `.octaveUp`, `.octaveDown`, `.shorter`, `.longer`, `.draw` and
`.tool1` … `.tool5`, which are read only
while the piano roll has the keyboard, so they can be changed like the rest. `pianoRoll.quantize`
(Ctrl+U) means nothing elsewhere, so it works whenever the piano roll shows a clip.

Later: Ctrl+G (glue), Ctrl+1 to Ctrl+4 (grid), Z (zoom to
selection), added to the keymap when built.

## Keymap presets

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** Settings › Shortcuts has
  a **Preset** menu with two choices, which set the keys of a few actions at once:

  | Preset | What it sets | Where it differs |
  |---|---|---|
  | **Select-first** (default) | the keys Suhina has today | duplicate is **Ctrl+D**; a right-click on a note does nothing |
  | **Draw-first** | the same, with the keys of someone who draws notes first | duplicate is **Ctrl+B**; in the Draw and Paint tools a right-click on a note deletes it |

- Choosing a preset sets the defaults of the keys it names; every other key keeps what it has, and
  single keys can still be changed afterwards, as today (› Settings in
  [`docs/shortcuts.md`](../shortcuts.md)). Choosing a preset twice sets them again, so a key that
  was changed by hand comes back to the preset's.
- The choice is saved with the keymap in `keymap.json` under `"preset"`, and read back when the
  file is read. A file without it is **Select-first**. A `"preset"` naming something else is
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
- **The fifth tool:** Slice, as built. If the maintainer wants another tool there, only the mapping
  of `pianoRoll.tool5` and the icon change.
- **The preset names** (**Select-first** and **Draw-first**) describe the habit; the maintainer may
  rename them ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)).
- Envelopes and Expression tabs (per-note pitch bend, pressure, timbre; clip automation): a
  separate spec.
- Audio clip editor (sample view, warp markers): a separate spec.
- MIDI recording into a clip, overdub and step input: belongs to recording.
