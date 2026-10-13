---
description: Measurements, states and tokens of every UI component, from the minimal v3 mockups
read_when: widget sizes, clip slot states, value field, slider, knob, toggle, meter, track header levels, device panel, piano roll layout
type: spec
status: draft
last_verified: 2026-10-13
covers:
  - crates/ui/src/theme.rs
  - crates/ui/src/session_view.rs
  - crates/ui/src/widgets.rs
  - crates/ui/src/arrangement_view.rs
  - crates/ui/src/timeline.rs
  - crates/ui/src/motion.rs
  - crates/ui/src/settings_view.rs
  - crates/ui/src/themes.rs
  - crates/ui/src/waveform.rs
  - crates/ui/src/detail_view.rs
  - crates/ui/src/project_dialog.rs
  - crates/ui/src/app.rs
---

# Components

Measurements, states and tokens of every UI component, taken from the minimal v3 mockups
([Session](reference/session-minimal-v3.png), [Arrangement](reference/arrangement-minimal-v3.png)).
Token names are defined in [`brief.md`](brief.md) › Tokens. Sizes are logical pixels at 100 %
scale. Rows marked **(proposed)** were not drawn in the mockups yet; use them until a design
replaces them.

## Global rules

- Radius 0 everywhere. No shadows, gradients or cards.
- Regions are separated by 1 px gaps that show `border` beneath them.
- Parts marked **(planned)** are designed but not built yet; everything else is what the app
  draws today.
- Type, by role:
  - 11 px `Encode Sans Semi Expanded` for UI text (names, labels of controls, menu items);
  - 10 px for small values and labels, and every number, time and position in `IBM Plex Mono`
    (bar numbers in the ruler too);
  - 9 px only for tags beside a name: a device's kind, a clip's gain, key labels in the clip
    editor;
  - 13 px only for a page title (Settings) and the drop overlay.
- Text colours: `text.primary` for values and names, `text.secondary` for labels,
  `text.hint` for inactive and hints. The one pure white (`#FFFFFF`) is the name of a playing
  or recording clip.
- Fonts live in `crates/ui/fonts/<font>/` with their licence and a row in `ATTRIBUTION.md`.

### Interaction states

| State | Change |
|---|---|
| Hover | Background one step lighter: `window.background` → `panel.background`, `panel.background` → `#2A2926`, `control.background` → `#34332F` |
| Pressed / dragging | `control.activeBackground` |
| Selected | A track header: `control.background` in the Arrangement view, `control.activeBackground` in the Session view; a clip: `control.activeBackground` with a 1 px `text.primary` inset |
| Keyboard focus | (planned) 1 px inset outline in `accent` |
| Disabled | Text `text.hint`, no fills |
| Track off (muted) | Its name in `text.hint`; its slots, clips, marks and colour strip at 55 % opacity |

### Motion

Animations are short and only show a change of state; with animations off (Settings ›
Animations, or `--no-animations` for one run) every value jumps straight to its end.

| What | How |
|---|---|
| Hover | Fill and text fade to the hover colour in 80 ms |
| States (playing scene, transport running, track on) | Fade in 120 ms |
| Queued clip | The accent outline breathes between 45 % and 100 % every 0.8 s |
| Values changed from elsewhere (undo, another client) | The fill eases to the new value in 120 ms; a drag follows the pointer directly |
| View switch | The underline slides to the chosen view in 160 ms |

## Window

From top to bottom: menu bar, tab row (when shown), control bar, the open prompt (when shown),
the view with the detail area, status bar.

| Region | Size |
|---|---|
| Menu bar | 20 px, `panel.background`, items 14 px apart in `text.secondary`: File (New or open…, Open…, Save, Save as…, Save as template…, Quit), Edit, View, Projects, Options; Create, Playback, Navigate and Help (planned) |
| Tab row | 22 px, `window.background`, shown while two or more projects are open (ADR 0007): one tab per project, name (with " •" while it has unsaved changes, as the window title has) and a 12 px × to close; the active tab `panel.background` with `text.primary`, others `text.secondary`; Ctrl+Tab and Ctrl+Shift+Tab move between them |
| Control bar | 28 px, `panel.background`, a 1 px `border` under it. Left, 6 px apart: tempo (56 × 20 value field, `control.background`, 11 px mono right-aligned, dragged up or down), time signature (40 px, 11 px mono `text.secondary`, "4 / 4"), launch quantisation (64 px dropdown, 11 px mono). Centre: play and stop (28 × 20 each, 1 px apart), the position (64 × 20 on `window.background`, 11 px mono, "bar.beat.sixteenth"). Right, left to right: the CPU load (56 × 20 on `window.background`, only while an engine plays: "CPU" in 9 px `text.hint` left and the smoothed percentage in 10 px mono `text.secondary` right, over a 2 px-tall bar of the load, `meter.normal` fill, `meter.hot` from 80 %, in a `control.background` track; the audio thread's share of wall time inside the engine's render call, per mille smoothed over 0.2 s windows, published as an atomic), then the view switch. (planned): metronome, record, loop |
| Transport button | 28 × 20, `control.background`; play shows `control.activeBackground` while playing, its glyph in `accent`; stop is an 8 × 8 square in `text.primary`; record (planned) a 1.5 px red ring |
| View switch | Text only, 11 px, right-aligned, 12 px apart: "Session", "Arrangement"; the current one `text.primary` with a 1 px underline, the other `text.secondary` |
| Open prompt | When a project is dropped while another is open and the setting says Ask: a 32 px row on `panel.background` above the view: "Open x:" In a new tab, In place of y, Cancel, Remember (which sets the setting) |
| Drop overlay | While a project file is dragged over the window: `window.background` at 45 % over the view and "Drop to open" in 13 px `text.primary`, fading in and out with the animations |
| Browser | 300 px wide by default, dragged from 200 to 600 px by a 4 px handle at its inner edge (`border`, `border.strong` under the pointer); left of the view, or right (Settings › Interface › Browser); Ctrl+B and View › Show browser hide and show it. See Browser below |
| Detail splitter | 6 px, `control.background`, a 32 × 2 grip in `text.hint` in the middle; the cursor shows which way it moves |
| Detail area | Under the view, 200 px by default, resizable from 80 px to all but 160 px of the view; Shift+Tab hides and shows it. Shows the selected track's device chain; a selected clip shows its properties and, beside them, the chain of the track it is on: in a row (220 px per device), or stacked in a column beside the view (260 px by default, resizable) — Settings › Interface › Detail area. With nothing selected: "Select a track or a clip to see it here" in `text.hint` |
| Status bar | 20 px, `panel.background`: the last message on the left (10 px, `text.secondary`, errors in the recording red), "N tracks · M scenes" on the right (10 px, `text.hint`). There is no separate info view |

## Browser

Behaviour in the [browser spec](../spec/browser.md). The panel is `panel.background`; its parts
from top to bottom:

| Part | Spec |
|---|---|
| Search | 26 px, `window.background`, 11 px search icon (a ring and a handle in `text.hint`), placeholder "Search", shortcut hint right in 9 px mono `text.hint` |
| Category filters | Wrapping row of buttons, 16 px high, 7 px side padding, 2 px gaps, 10 px text; selected `text.value` with `window.background` text, others `control.background` (hover `control.hoverBackground`) with `text.secondary` |
| Sidebar | 104 px, a 1 px `border` on its right; section titles (Pinned, Recent, Folders) 10 px `text.hint` with 8 px top padding; items 17 px, 10 px `text.secondary` (`text.primary` when selected); selected item `control.activeBackground`, hover `panel.hoverBackground`; a folder's state at its right in 9 px mono `text.hint` ("…" scanning, "poll", "gone"); "+ Add folder" in `text.hint` opens a path field in its place |
| Tree row | 17 px; indent 12 px per level; 10 px lead (a chevron in `text.hint` for what opens, a play triangle for a selected or hovered sample or MIDI file), 12 px type icon, 5 px, name 10 px `text.value` (`text.primary` selected), tag dots, meta right in 9 px mono `text.hint` (length, "plug-in", the folder a search found it in); selected row `control.activeBackground`, hover `panel.hoverBackground`; a 2 px `border.strong` bar at the right edge shows the scroll position |
| Type icons | Our own strokes, 1.1 px, 12 px: folder and heading `text.secondary`; MIDI note `track.2` (ochre); audio wave `track.3` (steel blue); instrument and preset `track.4`; effect `track.1`; project page `text.secondary` |
| Colour tag | 6 px dot after the name, 2 px apart, in the tag's colour |
| Preview strip | 54 px, `window.background`, a 1 px `border` on top: 22 px play button (`control.background`, its glyph `accent` while playing), file name 10 px, length, BPM ("~" when detected) and key right in 9 px mono `text.secondary`; under them a waveform of 1 px bars (a MIDI file: 2 px note bars), played part `text.secondary`, rest `text.hint`, playhead 1 px `accent` |
| List and Map | 22 px row above the results: two buttons like the category filters, List and Map; Map shows the [sample map](../spec/sample-map.md) in the results area |
| Sample map | A 24 px top bar (Cloud and Grid, the status in 10 px `text.secondary`, then +, −, Fit, Large at the right), the map on `window.background`, a 20 px legend under it (a 7 px dot and "Kick 150" in 10 px, per kind, in the kind's colour; dimmed kinds at 30 %). Points: 2–6 px dots by zoom (a 1.2 px ring for a preset), or square cells 1 px apart in the grid; a guessed kind at 70 % and dimmed points at 18 %; rings: in use 1.5 px in the kind's colour, selected 1 px `text.primary`, previewed or under the pointer 1.5 px `accent`, similar 1 px `accent`. Kind names 17.6 px in the kind's colour with a 1 px `window.background` shadow, fading out from 4× to 6× zoom. Show similar adds a 180 px `panel.background` list at the right. Kind colours are our own muted hues (kick `#D06E5E`, snare `#73AAC0`, clap `#5EC0B0`, closed hat `#C6AC5E`, open hat `#D08F6E`, …; `sample_map::kind_colour`) |
| Footer | 20 px, `panel.background`, a 1 px `border` on top: the tags as 6 px dots (a filter ring in `text.primary` when on), the count right in 9 px mono `text.hint` ("12 items", "3 selected") |
| Drag label | Beside the pointer while dragging from the browser: 10 px text on `control.activeBackground` with a 1 px `accent` outline when it can land (`border.strong` when not); the target is outlined 1.5 px `accent` (a 2 px `accent` mark at the beat on an Arrangement lane) |

## Session view

From left to right: the track columns (groups and their tracks in project order), empty space
(`window.background`) if any is left, the return columns pinned against the scene column, and the scene
column pinned to the right edge. The track columns scroll sideways when they do not fit; the
returns and the scene column stay. From top to bottom: header row, scene rows, the stop row, empty
space, the mixer at the bottom.

| Part | Spec |
|---|---|
| Track column | 116 px; return column 84 px; scene ("Main") column 96 px |
| Column separator | 1 px `border.strong` through header, grid and mixer |
| Row separator | 1 px `border` between scene rows, continued through returns and scenes |
| Track header | 20 px, `control.background` (`control.activeBackground` when selected), 2 px bottom strip in the track colour; name 11 px `text.primary` (`text.hint` when off). A click selects the track, a drag moves it, a right-click opens its menu |
| Main header | 20 px, `control.background`, "Main" in `text.primary`, no colour strip |
| Scene button | 20 px, `control.background` (hover `#34332F`), name left in 11 px, number right in 10 px mono `text.hint`; a playing scene `control.playingBackground` |
| Stop row | 20 px under the last scene, `panel.background`: per track a 7 × 7 square outline in `text.secondary` (`accent` while the stop is queued), centred; in the scene column the square and "Stop all" |
| Empty space | Right of the last track: "+ MIDI track" and "+ Audio track" buttons (20 px, `control.background`, 10 px `text.secondary`, 2 px apart, 6 px from its top left) and, below them, "Drop files or devices to add a track" in 10 px `text.hint`. Drops from the browser there make new tracks; the Arrangement has the same under its last lane |

Track colours: the track's own colour if it has one, else `track.1`–`track.4` in track order.

### Clip slot

20 px high, 6 px side padding, 6 px gap between mark and name.

| State | Background | Mark | Extra |
|---|---|---|---|
| Empty without a stop button | `window.background` | none | |
| Empty with a stop button | `window.background` | 3 × 3 `#4A4743` | |
| Stopped | `control.background`, text `#C9C4BB` | 6 × 6 track colour | |
| Queued | `control.background` | 6 × 6 track colour at 50 % with 1 px `accent` outline | 1 px inset `accent` outline on the cell |
| Playing | `control.playingBackground`, text `#FFFFFF` | 6 × 6 track colour | 2 px `accent` progress line along the bottom |
| Recording | (planned) `#3A2B2A`, text `#FFFFFF` | 6 px red dot | 2 px red progress line |

### Mixer strip (Session)

72 px high, `panel.background`, 6 px padding, under every track and return column; the scene column's
part is empty (the main track's strip is planned). From top to bottom:

| Part | Spec |
|---|---|
| Knob row | 22 px: the pan knob at the left. Sends (planned) as more knobs in the same row |
| Knob | 22 px; circle r 9, `control.background` fill, 1 px `text.hint` stroke; pointer 1.5 px from centre to r 8; travel −135° … +135°; pan pointer `text.primary`, sends `#C9C4BB` |
| Values row | 12 px, 4 px under the knobs: pan left (10 px mono `text.secondary`, "C", "15L"), volume right (10 px mono `#C9C4BB`, dragged up or down) |
| Toggles | 18 px at the bottom: track on (two thirds, the track number, or the return's letter, in 10 px mono) and solo (one third, "S" in 10 px), 2 px apart; arm (planned) |
| Track on | On: `#C9C4BB` with `window.background` text. Off: `control.background` with `text.hint` |
| Solo | On: `accent` with `window.background` text; off: `control.background`, `text.secondary`. A click solos the track alone; Ctrl (Cmd) adds it to the tracks soloed already |
| Arm | (planned) Off: 1.2 px `text.secondary` ring. On: `#3A2B2A` background, filled red dot |
| Meter | (planned) Two 4 px bars, 2 px apart, on `window.background`, beside the values; `meter.normal`, the top 12 % `meter.hot` when above −3 dB |

## Arrangement view

The timeline on one side, the track header column on the other (right by default). From top to
bottom: the ruler, the track lanes (scrolling), and fixed rows for the return tracks and the main
track along the bottom edge.

| Part | Spec |
|---|---|
| Bar ruler | 20 px, `panel.background`; bar numbers in 10 px mono `text.secondary`, 3 px right of a 1 px `border`, labelled every 1, 2, 4 … bars so labels are at least 40 px apart. A click sets the play start marker (`transport.setPosition`) to the beat under the pointer, on the clip grid of that zoom, Alt for free ([spec](../spec/arrangement-editing.md) › Play position) |
| Play start marker | A 9 × 6 px triangle in `text.primary`, pointing down, its tip on the ruler's lower edge at the marker's beat |
| Loop brace | (planned) a 14 px row under the ruler; 6 px bar `control.activeBackground` with 1 px `text.hint` inset |
| Zoom and moving | 8 px per beat by default, 1–400 px; Ctrl and the wheel zoom around the pointer, Alt and the wheel make every track lane taller or shorter (within 20–400 px) keeping the lane under the pointer in place, the wheel scrolls; holding the middle button down over the timeline moves it freely both ways (the cursor grabs), also over a clip, which stays where it is: clips, fades, fields and headers are dragged with the left button only |
| Grid lines | In the lanes and the fixed rows (not the empty space below the lanes): a 1 px `border` at each bar, and a 1 px `panel.background` line at each beat once beats are 10 px apart |
| Lanes | Default height 48 px, the lower edge of a header resizes it (20–400 px); separated by 1 px `border.strong`; empty area `window.background`. Overlapping (stacked) clips share the lane in equal sub-rows |
| Clip | Fills its lane (or sub-row) from top to bottom, 1 px short of its end; `control.background` body; 2 px top strip in the track colour; name 10 px `#C9C4BB`, 4 px from the left and 3 px from the top (it stays in view while the clip's start is scrolled off); selected clip `control.activeBackground` with 1 px `text.primary` inset; a drag moves it along its lane or to another |
| MIDI marks | Under the name: one 2 px line per note in the track colour, its length the note's; the clip's lowest note at the bottom, its highest at the top |
| Audio marks | The waveform: one 1 px vertical line per pixel column in the track colour, from the lowest to the highest sample it covers (the engine's beat-to-file mapping: offset, loop, warp markers), scaled by the clip gain and its fades (both as they are dragged, before the edit lands); a centred line until the file is decoded. Clips at least 80 × 30 px show their gain 12 px from the top right (44 × 12 px, 9 px mono, "-3.0 dB"), dragged up or down (`clip.setGain`, −70 … +24 dB, double-click resets) |
| Fades | On an audio clip: a 1 px `text.secondary` line from the bottom-left corner to where the fade-in ends at the top (2 px down, under the colour strip), and from where the fade-out starts to the bottom-right corner |
| Fade handles | 7 × 7 px squares at the clip's top corners, inside the clip, shown while it is hovered or selected (and while dragged) on clips at least 21 × 14 px; with a fade, at the fade's end along the top edge. `text.secondary`, `text.primary` hovered, 1 px `window.background` outline; an 11 × 11 px area takes the pointer, drawn and hit above the gain field. A horizontal drag sets the fade (`clip.setFades`): the fade's end snaps to the clip grid, Alt for free; double-click removes it |
| Playhead | 1 px `accent` through the ruler and all lanes while playing; while stopped, at the play start marker once it is past beat 0 |
| Header column | 232 px by default, dragged wider or narrower by its inner edge (160–520 px); on the right of the timeline, or the left (Settings, or right-click above the headers); `panel.background` (`control.background` when selected), 5 px top/bottom and 6 px side padding (8 px more per group level), 6 px between groups. A click selects the track (Ctrl adds), a drag moves it, a right-click opens its menu (name field, colour swatches, group, ungroup, delete) |
| Return and main rows | 20 px each along the bottom, `panel.background` headers: the name in 11 px and the volume as a plain 10 px mono value on the right (dragged up or down; the main track's is shown only) |

### Track header levels

The header reveals groups as the track grows. Thresholds are the heights at which a group fits;
a lane between 20 and 26 px shows the first row clipped.

| Track height | Shows |
|---|---|
| ≥ 26 px | Row 1, 15 px: track on (30 px, the track number in 10 px mono), solo (18 px, "S"), a 6 px colour square, the name in 11 px; arm (planned) |
| ≥ 48 px | Row 2, 6 px under row 1: the volume field (the rest of the row) and the pan field (46 px), 4 px apart |
| ≥ 70 px | Routing, 4 px under row 2: "→" and the output dropdown ("Main" or a group track, `track.setOutput`); input and channel dropdowns and monitor In / Auto / Off (planned, with recording) |
| ≥ 114 px | (planned) Send fields, one per return, labelled "Send A", "Send B", once the engine plays sends; routing then moves below them |

### Value field (slider)

A number field that is its own slider: drag vertically to change (Shift for fine steps),
double-click to reset (typing a value is planned).

| Part | Volume | Pan | Send |
|---|---|---|---|
| Size | 16 px high, full width | 16 × 46 | 16 px, half width each |
| Background | `control.background` | `#232220` | `control.background` |
| Value shown as | Fill from the left, `#4A4843`, full height | 2 px bar along the bottom from the centre, `#C9C4BB` | Fill `#3E3C38` |
| Marker | 1 px tick in `#C9C4BB`, only the top and bottom 2 px | 1 px centre tick `#55524C`, top and bottom 2 px | none |
| Text | Value right-aligned, 10 px mono | Centred | Label left (sans), value right (mono) |
| Mapping | 0 dB at 85 % of the travel | 50L … C … 50R | as volume |

### Dropdown and segmented control

| Part | Spec |
|---|---|
| Dropdown | 15 px high, `control.background`, 4 px padding, 10 px text `#C9C4BB`, 6 × 4 chevron `text.hint` right |
| Segmented | 15 px, equal parts 1 px apart (64 px each in Settings); selected `#C9C4BB` with `window.background` text, others `control.background` with `text.secondary`, 10 px text |

## Settings page

Options › Settings (Ctrl+,) replaces the main view until it is closed (Close or Escape); there are
no dialogs. A centred block at most 724 px wide on `window.background`: the title "Settings" in 13 px
with a 52 px Close button on the right; under it the list of pages (140 px), 24 px of space, and
the page (560 px). A change applies at once and is saved; one that needs a restart (the
renderer) says so in its note.

The list of pages: 20 px rows, 11 px names 6 px in, `text.secondary`; the page shown is filled
`control.background` with `text.primary`, a hovered one `#2A2926`. Interface's own pages are indented
12 px more under it:

- General
- Interface
  - Theme
  - Arrangement
  - Detail area
  - Browser
- Graphics
- Audio
- Shortcuts
- Plug-ins

A page starts with its title (10 px `text.hint` over a 1 px `border`), level with the top of
the list; each setting is a 34 px row with its name (11 px `text.primary`), a note under it
(10 px `text.secondary`) and its control on the right, a segmented control of 64 px per part. A
read-only setting (Output) shows its value as the note.

| Page | Setting | Values |
|---|---|---|
| General | When opening a project | Ask (default), New tab, Replace |
| General | Projects folder, Templates folder | A 300 px field each; empty uses `Documents/Suhina/Projects` and `…/Templates`, shown as the hint |
| General › Dropping files | Several files | One track (default), One each; Ctrl while dropping does the other ([browser spec](../spec/browser.md) › Files) |
| General › Dropping files | Warp mode | Beats (default), Tonal, Texture, Smooth, Pro, Re-pitch: how a dropped sample with a known tempo is stretched |
| General › Transport | Stop | Marker (default): back to the play start marker; Stay: the marker moves to where it stopped ([spec](../spec/arrangement-editing.md) › Play position) |
| Interface | Animations | On (default), Off |
| Interface | Zoom | Three parts: −, the value ("100 %", a click goes back to 100 %), +; 50–200 % in 10 % steps, the size of the whole window ([spec](../spec/window-zoom.md)) |
| Interface › Arrangement | Track headers | Left, Right (default) |
| Interface › Arrangement | Allow sample stacking, Allow MIDI stacking | On, Off (default): moved clips stack instead of replacing ([spec](../spec/arrangement-editing.md)) |
| Interface › Detail area | Device chain | Row (default), Stack |
| Interface › Browser | Side | Left (default), Right |
| Interface › Browser | Auto-play | On (default), Off: selecting a sample or MIDI file previews it |
| Interface › Browser | Preview volume | −18, −12, −6 (default), 0 dB |
| Interface › Browser | Tag limit | 64 (default), 128, 256, 1024 colour tags (`library.setTag`'s `limit`) |
| Interface › Browser | Map layout | Cloud (default), Grid ([sample map](../spec/sample-map.md)) |
| Interface › Browser | Map preview on hover | On (default), Off |
| Interface › Browser | Map: longest sample | 5, 10 (default), 30, 60 s |
| Graphics | Renderer | GPU (wgpu, default), OpenGL (glow) |
| Audio | Output | The device in use (read only) |

**Interface › Theme**: the themes as 20 px rows in a list that scrolls after eight, the user's
own first and then those that come with Suhina, the name on the left and the file name or
`built-in` on the right (10 px `text.hint`); a click uses one, the one in use filled like the page
list and scrolled into view when the page opens. Under them a 34 px row: the name of the theme in
use in a field, kept on Enter or leaving it (a built-in theme shows its name and "changing a
colour makes a copy"), and the folder of the theme files as the note. Then a row of buttons
(15 px, as wide as their text): Duplicate, Reload, Open folder, Back up, Export template. Then
"Colours" over a hairline and every colour token in a list that scrolls: an egui colour button
with a 1 px `border.strong` outline, the value and the token name (10 px mono), and what it is
used for. Changing a colour of a built-in theme first makes a copy of it ("<name> (custom)").
See [brief](brief.md) › Themes.

**Shortcuts** lists every action under its group (Transport, View, Edit, Tracks, Commands): its
label (300 px, 10 px), its keys (130 px, 10 px mono `#C9C4BB`, "Press keys" in `accent` while
waiting), Change and Clear; Reset all at the top; a key bound twice in red above the list
([shortcuts](../shortcuts.md)).

**Plug-ins**: an "Editors" row first (Window · Device view: where a plug-in's editor opens; the
note says when the device view is not available on this system); then "Folders" (11 px), the
standard folders in 10 px `text.hint` with "(standard)",
the added ones in `text.secondary` with a 64 px Remove button, a 20 px field to add one with a
64 px Add button; a 34 px Scan row (the count found and failed as its note, Scan and Rescan all
on the right, run in the background); then the plug-ins found (name, vendor · format version) and
the files that failed (in the recording red), in a list that scrolls. Read and changed through
the `plugins.*` commands.

The app keeps them in `settings.txt` (`key = value` lines) in its app-data folder, the theme in
use as `theme = <id>`; unknown lines are skipped with a warning in the status bar.

## Project dialog and save prompt

File › New or open (Ctrl+T) and Open (Ctrl+O, the path field takes the keys) show a dialog over
the window, as the plug-in loading dialog: the window dimmed with `window.background` at 60 %,
a 620 × 380 px panel, `panel.background` with a 1 px `border.strong` border, a 20 px
`control.background` title "New or open a project" with Close on the right. Escape closes it;
shortcuts wait while it shows.

| Part | Spec |
|---|---|
| New (left half) | "New" heading (10 px `text.hint` over a `border` hairline); a 20 px row "New blank project" with "4 tracks, 2 returns, 8 scenes" on the right; "From a template" and the templates folder's `.locr` and `.als` files by name, 20 px rows that scroll, "Suhina project" or "Live Set" on the right; "No templates yet: File › Save as template" when there are none; the folder at the bottom in 10 px `text.hint` |
| Open (right half) | A path field (hint "Path to a .locr or .als file"; Enter opens; quotes around a pasted path are dropped) and a 56 px Open button; "Recent": the projects opened and saved lately, newest first (12 kept), the name and its folder, or "Nothing opened yet" |
| Rows | Hover `panel.hoverBackground`; name 11 px `text.primary`, note 10 px `text.hint` |

A new project opens in a new tab. Save (Ctrl+S) writes the project's own `.locr` file; the first
time, Save as (Ctrl+Shift+S) and Save as template ask in a 420 px prompt: Name and Folder fields
(the projects folder, the project's own folder, or the templates folder), the file that will be
written under them (in the recording red when it is there already and will be replaced), Cancel
and Save (Enter). A name with `/`, `\` or `:` cannot be saved.

## Device panel

| Part | Spec |
|---|---|
| Panel | `panel.background`, panels 1 px apart, end of chain is empty `window.background` |
| Header | 20 px, `control.background`, 6 px padding: on switch, name, and what it is on the right in 9 px `text.hint` (Suhina, CLAP, VST3, VST2, or "Not supported" for a device of another DAW) |
| On switch | 9 × 9 square, 1 px border; on: filled `text.primary`; off: outline `text.hint`, name `text.hint` |
| Parameter | A 16 px row: its id in 10 px `text.secondary` on the left, its value in 10 px mono `#C9C4BB` on the right. (planned, with the device commands and parameter ranges) a 44 px column: 26 px knob (r 11, pointer to r 10), value 10 px mono, label 10 px `text.secondary`; 14 px between columns; pointer `accent` when the device is on |
| Plug-in | A 16 px row with where it stands, 10 px: the plug-in's name and vendor (and its latency in frames) in `text.secondary` while it runs; "Loading …", "Not running" (the project is not playing) or "Not installed; kept with its settings" in `text.hint`; "Crashed: …" or "Could not start: …" in `recording`. Under it a 96 × 18 px button, `control.background` (hover `control.hoverBackground`), 1 px `border.strong` border, 10 px `text.primary`: "Open editor" or "Close editor" while it runs, "Restart" after a crash; none otherwise. It runs `device.openEditor`, `device.closeEditor` or `device.restart` |
| No parameters | Another DAW's device: "Kept so the chain keeps its shape"; a built-in effect or instrument without parameters: "No parameters" (10 px `text.hint`) |
| Stacked | In a column, each device is as tall as its parameters (20 px header, 6 px padding, 16 px a parameter; a plug-in as tall as its state row and button), 1 px apart |

## Plug-in loading dialog

Shown over the window while the active project's plug-ins start (`crates/ui/src/loading_dialog.rs`),
until none is loading or it is sent to the background; a project opened next shows it again.

| Part | Spec |
|---|---|
| Behind | The window dimmed with `window.background` at 60 %; clicks there do nothing |
| Panel | 360 px wide, centred, `panel.background`, 1 px `border.strong` border; 20 px `control.background` title "Loading plug-ins · <project>" in 11 px `text.primary` |
| Progress | A 4 px bar, `control.background`, filled with `accent` for the share ready; under it 16 px rows in 10 px: "N of M ready, K not installed" `text.secondary`, "Loading <name>" `text.hint` (six, then "and N more"), "Could not start <name>" `recording` |
| Button | 150 × 18 px, bottom right, as the device panel's: "Continue in the background"; Escape does the same. The project plays meanwhile |

## Clip properties

The detail area for a selected arrangement clip: a 196 px `panel.background` column with the clip's name
(20 px title, 11 px) and 16 px rows of label (10 px `text.secondary`) and value (10 px mono
`#C9C4BB`): Track, Start (bar.beat), Length (bars.beats), Loop, and Gain for an audio clip or
Notes for a MIDI clip; a value keeps 52 px clear of its label, so a long track name is shortened
rather than drawn over it. The track's device chain follows to the right (in a row) or below (stacked:
the column is then as tall as its rows). Editing them, and the clip editor beside them, are planned.

## Clip editor (MIDI, planned)

| Part | Spec |
|---|---|
| Columns | Clip properties 196 px · tools 112 px · editor |
| Clip properties | 20 px title with 2 px clip-colour strip; 6 px padding and group gap; each pair = labels row (10 px `text.secondary`) and values row (15 px mono fields); Loop as a 15 px toggle |
| Tools | Transpose field, then a two-column grid of 18 px buttons (×2, ÷2, Fit scale, Invert, Humanize, Quantize, Reverse, Legato) |
| Toolbar | 20 px: tabs (current has a 1 px underline), divider, Fold, Scale (on = `#C9C4BB`), scale name, grid value right |
| Ruler | 16 px; bar numbers `#C9C4BB`, beats `text.hint`; bar lines `#5A5751`, beat lines `#34332F` |
| Clip loop | 8 px row, 4 px bar `control.activeBackground`, 2 × 8 handles `text.primary` |
| Key column | 36 px; white keys `control.background`, black keys `window.background`; 2 px scale mark `#6E6A63`; C labels with octave (C3 = MIDI 60) |
| Note rows | Scale rows `#23221F`, others `#1A1917`; 1 px row lines `#2A2926`, under C `border.strong` |
| Note | Track colour; opacity 0.4 + 0.6 × velocity / 127; name in 9 px when ≥ 2 beats wide; 1 px gap to neighbours; selected = 1 px `text.primary` inset |
| Velocity lane | 72 px default (resizable); 36 px scale column (127, 64, 1); one 1 px stem per note with a 5 × 2 head, chord notes 4 px apart; selected stem `text.primary` |
| Velocity tools | 18 px: Randomize, Ramp, Deviation as small value fields |
