---
description: Functional spec of the MIDI clip editor: resizable detail area, zoom, regions and editing
read_when: piano roll, clip editor, velocity lane, zoom, detail area height, note editing
type: spec
status: planned
last_verified: 2026-10-13
---

# UI: clip editor (MIDI)

Functional spec of the editor that opens in the detail area for a MIDI clip, written from using
Live 12.4.6. Only structure and behaviour; the look comes from
[`docs/design/brief.md`](../design/brief.md). Items marked **(to observe)** still need checking.

Status: planned; nothing of the editor is built yet. The detail area's splitter exists with one
height, not saved per view (`crates/ui/src/app.rs`).

## Size and navigation

The editor is built to be used at any size, from a strip under the arrangement to most of the
window.

- **The detail area is resizable.** A horizontal splitter between the main view and the detail
  area is dragged up or down; the editor can take nearly the whole window height. The height is
  to be saved per view (planned; today one height for every view, kept while the app runs).
- **Everything scales with it.** Key rows, the note grid and the velocity lane grow and shrink
  with the available height; nothing is fixed to one size.
- **Vertical zoom** changes the height of key rows (more or fewer octaves visible). **Horizontal
  zoom** changes beats per pixel. Both zoom around the pointer; dragging in the ruler zooms
  horizontally **(to observe: exact gestures and modifier keys)**.
- **Scrolling:** wheel scrolls vertically, with a modifier horizontally. Fold and zoom-to-fit
  keep the scroll position sensible.
- **Grid** follows zoom (adaptive) or is fixed (1/16 etc.); notes snap to it unless snapping is
  turned off with a modifier.

## Regions

1. **Clip properties** (left, collapsible): clip name and colour; start and end; loop on/off,
   loop position and length; time signature; groove; scale root and mode.
2. **MIDI tools** (collapsible): transpose, ×2 and ÷2 (stretch), fit to scale, invert, humanize,
   quantize, reverse, legato, set length.
3. **Editor toolbar:** tabs for Notes, Envelopes and Expression; Fold (show only keys that have
   notes); Scale (highlight the clip's scale); grid value.
4. **Ruler** with bar and beat numbers, and the clip's **loop region** with draggable start and end.
5. **Key column**: piano keys; every C labelled with its octave (Live names MIDI 60 "C3").
   Keys in the selected scale are marked.
6. **Note grid**: one row per key, scale rows distinguishable from the others, bar lines stronger
   than beat lines. Notes show their name when wide enough; velocity is visible on the note.
7. **Velocity lane** (resizable height): one stem per note at its start; chord notes on the same
   start sit side by side. Tools below: randomize, ramp, deviation.

## Interaction

- Double-click to add or delete a note; drag to move; drag an edge to change length.
- Drag a velocity stem to change velocity; with several notes selected, all change together.
- Arrow keys move the selection by grid steps (up/down by semitone); a modifier moves by octave.
- Selection by rectangle; copy, paste, duplicate (Ctrl+D) work on notes.

## Open

- Envelopes and Expression tabs: separate spec.
- Audio clip editor (sample view, warp markers): separate spec.
