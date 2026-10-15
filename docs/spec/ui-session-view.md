---
description: Functional spec of the main window and the Session view: regions, clip slots, scenes, I/O, mixer, browser
read_when: Session view, main window layout, clip slot behaviour, scene launch, mixer section, browser structure
type: spec
status: draft
last_verified: 2026-10-15
covers:
  - crates/ui/src/session_view.rs
  - crates/ui/src/grid.rs
  - crates/ui/src/app.rs
  - crates/ui/src/widgets.rs
  - crates/ui/src/detail_view.rs
  - crates/engine/src/cpu.rs
---

# UI: main window and Session view

Functional spec, written from using Live 12.4.6 (Suite, Windows) with a self-authored set. Only
structure and behaviour; colours, sizes and look come from [`docs/design/brief.md`](../design/brief.md).
Items marked **(to observe)** still need checking in Live.

## Window regions

From top to bottom:

1. **Menu bar:** File, Edit, Create, View, Playback, Navigate, Options, Help. (Suhina has File,
   Edit, View, Projects, Options and Help so far; components.md lists what is built.)
2. **Control bar**, left to right:
   - external sync and tempo: sync toggle, tap tempo, tempo field, nudge, time signature
   - groove amount, metronome, global launch quantisation
   - key and scale
   - transport: arrangement position, play, stop, record, overdub, automation arm, loop region
     start, punch, loop, loop length. **Suhina:** play starts from the play start marker and
     stop returns to it or stays, as Settings › General › Stop says
     ([arrangement editing](arrangement-editing.md) › Play position)
    - draw mode, computer-keyboard MIDI, MIDI and audio activity, sample rate, CPU load
      (**Suhina:** the CPU load is built, left of the view switch; `launcher.state`
      reports it as `cpu`; the rest is not). Left of the CPU load, the program's own RAM
      (its resident memory, in MB or GB), shown always
    - view switches (Session / Arrangement). **Suhina:** the last control at the right end
      of the bar, with the CPU load and the RAM left of it. In a window too narrow for all
      of them the RAM, then the CPU load, is left out rather than overlap the position
      readout; the order stays
3. **Main area**, left to right:
   - **Browser** (collapsible, resizable width). Observed in Live: a search field; a sidebar of
     collections (user colour tags), library categories and places (user folders); a results
     tree; tags and an item count below it. **Suhina's structure** (see the design brief):
     search with a shortcut; category filters as buttons (all, samples, MIDI, instruments,
     effects, presets, plug-ins); a sidebar of places only (Pinned, Recent, Folders, add folder);
     a file tree with expandable folders, type icons and length or duration; colour tags as dots
     on rows; a preview strip with play, waveform, playhead, duration, BPM and key; tags and the
     selection count at the bottom. **Suhina:** how it behaves (places, search, preview,
     dragging into the project) is in [`browser.md`](browser.md).
   - **Groove pool** under the browser (collapsible).
   - **Session view** (or Arrangement view; Tab switches).
4. **Detail area** (collapsible), across the full width: the selected track's device chain, or
   the selected clip's editor.
5. **Info view** in the bottom left corner: help text for whatever is under the pointer.
6. **Status bar:** messages, the selected track's name and a mini overview.

Suhina keeps one 20 px status bar for messages and counts and has no separate info view; where
this list and `docs/design/components.md` differ, components.md describes Suhina.

## Session view

A grid. **Columns are tracks** in project order: regular tracks, then a gap, then return tracks,
then the main track at the far right. **Rows are scenes.** The main track's column holds the scene
launch buttons, numbered from 1; an extra empty area below the last scene accepts drops to
create a new scene or track.

### Track header

Track name over a strip in the track colour (Live colours the name itself). Clicking selects the
track; dragging moves it; right-clicking opens its menu (rename, colour, group, ungroup, delete).
Double-clicking to rename is planned.
Dropping a device or file from the browser on the empty area right of the last track creates a
new track; the area also has **+ MIDI track** and **+ Audio track** buttons ([browser](browser.md) ›
Files). A file dropped on a slot or below the stop row becomes a clip in that slot (scenes are
added below when needed); a device dropped on a column joins the track's chain. Samples and MIDI
files dragged from the system's file manager land the same way ([browser](browser.md) › Files
from the system).

### Clip slots

Each cell is a slot for one clip.

| Slot state | Shows | Click |
|---|---|---|
| Empty, with stop button | stop control | stops the track at the next quantisation point |
| Empty, without stop button | nothing | nothing |
| Clip, stopped | clip colour, clip name, launch control | launches the clip |
| Clip, queued | launch control in a pending (blinking) state until the quantisation point | |
| Clip, playing | launch control in the playing state, progress through the clip | relaunches |
| Clip, recording | recording state | |

Double-clicking an empty slot on a MIDI track creates an empty one-bar clip and opens it in the
detail area. Double-clicking a clip opens it. Dropping a file or clip on a slot puts the clip
there; on an empty audio track the track takes the clip's name **(observed in the corpus)**.

Below the last scene each track has a **stop all clips** button for that track, and the main
column has **stop all clips** for the whole set.

### Scene launch (main track column)

One launch control per scene with its number or name. Launching a scene launches every clip in
its row and stops tracks whose slot in that row is empty with a stop button. Scenes can carry a
tempo and time signature that apply when launched.

### I/O section (below the grid, per track, toggleable)

- MIDI tracks: "MIDI From" (source, channel), monitoring (In / Auto / Off), "MIDI To" or "Audio
  To" once an instrument is on the track.
- Audio tracks: "Audio From" (source, channel), monitoring, "Audio To".
- Return tracks: "Audio To".
- Main: cue out and main out.

### Sends (toggleable)

One knob per return track (A, B, …). Return tracks show pre/post toggles on the main column.

### Mixer (toggleable, resizable height)

Per track, top to bottom: pan, volume with numeric field and meter, track on/off (shows the
track number), solo, arm (MIDI and audio tracks). **Solo:** while any track is soloed, only the
soloed tracks are heard, with the groups they play into, the tracks inside a soloed group, and
the return tracks (so effects stay audible); mute still wins. A click solos one track and ends
the other solos; with a modifier (Ctrl) it adds to them (`track.setSolo`, `exclusive`). MIDI tracks without an instrument show no
volume, pan or sends, only the MIDI meter. The height of this section is a saved view setting
**(observed: `ViewStateSessionMixerVolumeSectionHeight`)**.

## Detail area: device chain

Devices left to right in chain order. Each device has a header (on/off switch, name, fold,
options) and its controls. Instruments come first on MIDI tracks; effects follow. An empty
area at the end accepts dropped devices. Plug-ins show their name and an option to open their
own window. Suhina shows where the plug-in stands (loading, running, missing, crashed) and a
button that opens or closes its editor, or restarts it after a crash
([components](../design/components.md) › Device panel). With the setting Plug-ins › Editors ›
Device view, the editor opens inside the panel under that line: in a row the device grows as wide as the
editor and the panel as tall as the tallest open editor; stacked, the device grows as tall as
its editor, which is cut off at the column's right edge; what does not fit is cut off at the
panel's edge, and the editor goes with its track (shown only while the track is).

## Shortcuts (to observe and list)

Tab (Session/Arrangement), Ctrl+D (duplicate), Ctrl+T / Ctrl+Shift+T / Ctrl+Alt+T (insert audio /
MIDI / return track), Ctrl+I (insert scene), Ctrl+Shift+I (capture and insert scene), Space
(play/stop), Enter (launch selected), arrow keys (move selection), 0 (deactivate) **(to observe:
full list)**.

**Suhina:** its keys and how to change them are in [shortcuts](../shortcuts.md); Ctrl+T there
opens New or open, not a new track.

## Open

- Clip editor (piano roll, sample editor): separate spec.
- Arrangement view: separate spec.
- Hot-swapping a device or sample from the browser ([browser](browser.md) › Not decided yet).
