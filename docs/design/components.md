---
description: Measurements, states and tokens of every UI component, from the minimal v3 mockups
read_when: widget sizes, clip slot states, value field, slider, knob, toggle, meter, track header levels, device panel, piano roll layout
type: spec
status: draft
last_verified: 2026-10-12
covers:
  - crates/ui/src/theme.rs
  - crates/ui/src/session_view.rs
  - crates/ui/src/widgets.rs
  - crates/ui/src/arrangement_view.rs
  - crates/ui/src/timeline.rs
  - crates/ui/src/motion.rs
  - crates/ui/src/settings_view.rs
---

# Components

Measurements, states and tokens of every UI component, taken from the minimal v3 mockups
([Session](reference/session-minimal-v3.png), [Arrangement](reference/arrangement-minimal-v3.png)).
Token names are defined in [`brief.md`](brief.md) › Tokens. Sizes are logical pixels at 100 %
scale. Rows marked **(proposed)** were not drawn in the mockups yet; use them until a design
replaces them.

## Global rules

- Radius 0 everywhere. No shadows, gradients or cards.
- Regions are separated by 1 px gaps that show `line` beneath them.
- Type: 11 px `Encode Sans Semi Expanded` for UI text; 10 px for small values and labels;
  a monospace for every number, time and position (egui's built-in one until `IBM Plex Mono`
  is bundled); 9 px only in rulers and key labels. Fonts live in `crates/ui/fonts/` with their
  licence and a row in `ATTRIBUTION.md`.
- Text colours: `text.primary` for values and names, `text.secondary` for labels,
  `text.tertiary` for inactive and hints.

### Interaction states (proposed)

| State | Change |
|---|---|
| Hover | Background one step lighter (`surface.2` → `#34332F`, `surface.1` → `#2A2926`) |
| Pressed / dragging | `surface.3` |
| Keyboard focus | 1 px inset outline in `accent` |
| Disabled | Text `text.tertiary`, no fills |
| Track off (muted) | The whole header and its lane at reduced contrast: text `text.tertiary`, fills `#34322F`, lane opacity 0.55 |

### Motion

Animations are short and only show a change of state; with animations off (a setting, and
`--no-animations` until Settings exists) every value jumps straight to its end.

| What | How |
|---|---|
| Hover | Fill and text fade to the hover colour in 80 ms |
| States (playing scene, transport running, track on) | Fade in 120 ms |
| Queued clip | The accent outline breathes between 45 % and 100 % every 0.8 s |
| Values changed from elsewhere (undo, another client) | The fill eases to the new value in 120 ms; a drag follows the pointer directly |
| View switch | The underline slides to the chosen view in 160 ms |

## Window

| Region | Size |
|---|---|
| Menu bar | 20 px high, items 14 px apart, `text.secondary`; File, Edit, View, Options so far |
| Control bar | 28 px high, three groups on `surface.1`: tempo and meter left, transport centre, status and view switch right |
| Transport button | 28 × 20, `surface.2`; play shows `surface.3` while playing, glyph in `accent`; record glyph is a 1.5 px red ring |
| View switch | Text only; current view `text.primary` with a 1 px underline |
| Browser | 300 px wide (resizable) |
| Detail splitter | 6 px, `surface.2`, 1 px `line.strong` above and below, 32 × 2 grip in `text.tertiary`; cursor `ns-resize` |
| Detail area | 150–300 px by default, resizable to nearly full height |
| Status bar | 20 px, hint text left, selection right |

## Browser

| Part | Spec |
|---|---|
| Search | 26 px, `surface.0`, 11 px search icon, placeholder "Search", shortcut hint right in 9 px mono |
| Category filters | Wrapping row of buttons, 16 px high, 7 px side padding, 2 px gaps; selected `#C9C4BB` with `surface.0` text |
| Sidebar | 104 px; section titles 10 px `text.tertiary` with 8 px top padding; items 17 px; selected item `#3A3936` |
| Tree row | 17 px; indent 12 px per level; 8 px chevron, 12 px type icon, name, meta right in 9 px mono |
| Type icons | Folder `text.secondary`; MIDI note `track` ochre; audio wave `track` steel blue; stroke 1.1 |
| Colour tag | 6 px dot after the name |
| Preview strip | 54 px: 22 px play button, file name, duration, BPM, key; waveform of 1 px bars, played part `text.secondary`, rest `#55524C`, playhead 1 px `accent` |
| Footer | 20 px: tags, selection count |

## Session view

| Part | Spec |
|---|---|
| Track column | 116 px; return column 84 px; scene column 96 px |
| Column separator | 1 px `line.strong` through grid and mixer |
| Row separator | 1 px `line` between scene rows, continued through returns and scenes |
| Track header | 20 px, `surface.2`, 2 px bottom strip in the track colour; muted track name `text.tertiary` |
| Scene button | 20 px, name left, number right in 10 px mono `text.tertiary`; playing scene `#4A4843` |

### Clip slot

20 px high, 6 px side padding, 6 px gap between mark and name.

| State | Background | Mark | Extra |
|---|---|---|---|
| Empty | `surface.0` | 3 × 3 `#4A4743` | |
| Stopped | `surface.2`, text `#C9C4BB` | 6 × 6 track colour | |
| Queued | `surface.2` | 6 × 6 track colour at 50 % with 1 px `accent` outline | 1 px inset `accent` outline on the cell |
| Playing | `#4A4843`, text white | 6 × 6 track colour | 2 px `accent` progress line along the bottom |
| Recording | `#3A2B2A`, text white | 6 px red dot | 2 px red progress line |

### Mixer strip (Session)

6 px padding, `surface.1`.

| Part | Spec |
|---|---|
| Knob | 22 px; circle r 9, `surface.2` fill, 1 px `text.tertiary` stroke; pointer 1.5 px from centre to r 8, round cap; travel −135° … +135°; sends `#C9C4BB`, pan `text.primary` |
| Values row | Pan left, volume right, 10 px mono |
| Toggles | 18 px high in one row: track on (flex 2, shows the track number), solo (flex 1), arm (flex 1) |
| Track on | On: `#C9C4BB` with `surface.0` text. Off: `surface.2` |
| Solo | On: `accent` with `surface.0` text |
| Arm | Off: 1.2 px `text.secondary` ring. On: `#3A2B2A` background, filled red dot |
| Meter | Two 4 px bars, 2 px apart, on `surface.0`; `meter.ok`, the top 12 % `meter.hot` when above −3 dB |

## Arrangement view

| Part | Spec |
|---|---|
| Bar ruler | 20 px; 10 px mono bar numbers; 1 px `line` at each bar |
| Loop brace | 14 px row; 6 px bar `surface.3` with 1 px `text.tertiary` inset |
| Lanes | Separated by 1 px `line.strong`; empty area `surface.0`; default height 48 px, the lower edge of a header resizes it (20–400 px) |
| Clip | `surface.2` body; 2 px top strip in the track colour; name 10 px `#C9C4BB` at 4/5 px; selected clip `surface.3` with 1 px `text.primary` inset |
| MIDI marks | 2 px high lines in the track colour |
| Audio marks | 1 px vertical lines in the track colour, centred |
| Playhead | 1 px `accent` through all lanes |
| Header column | 232 px by default, dragged wider or narrower by its inner edge (160–520 px); on the right of the timeline, or the left (Settings, or right-click above the headers); `surface.1`, 5 px top/bottom and 6 px side padding, 6 px between groups |

### Track header levels

The header reveals groups as the track grows. Thresholds are the heights at which a group fits.

| Track height | Shows |
|---|---|
| ≥ 26 px | Row 1: track on (30 px, the track number), 6 px colour square, name; solo and arm (18 px) to come |
| ≥ 48 px | Volume field (full width) and pan field (46 px) |
| ≥ 70 px | Send fields, one per return, labelled "Send A", "Send B" |
| ≥ 114 px | Routing, with 4 px extra space above it: input and channel dropdowns, output dropdown ("→ Main"), monitor In / Auto / Off |

Return and main headers are single 20 px rows: name and volume.

### Value field (slider)

A number field that is its own slider: drag vertically to change, double-click to type.

| Part | Volume | Pan | Send |
|---|---|---|---|
| Size | 16 px high, full width | 16 × 46 | 16 px, half width each |
| Background | `surface.2` | `#232220` | `surface.2` |
| Value shown as | Fill from the left, `#4A4843`, full height | 2 px bar along the bottom from the centre, `#C9C4BB` | Fill `#3E3C38` |
| Marker | 1 px tick in `#C9C4BB`, only the top and bottom 2 px | 1 px centre tick `#55524C`, top and bottom 2 px | none |
| Text | Value right-aligned, 10 px mono | Centred | Label left (sans), value right (mono) |
| Mapping | 0 dB at 85 % of the travel | 50L … C … 50R | as volume |

### Dropdown and segmented control

| Part | Spec |
|---|---|
| Dropdown | 15 px high, `surface.2`, 4 px padding, 10 px text `#C9C4BB`, 6 × 4 chevron `text.tertiary` right |
| Segmented (monitor) | 15 px, three equal parts 1 px apart; selected `#C9C4BB` with `surface.0` text |

## Settings page

Options › Settings (Ctrl+,) replaces the main view until it is closed (Close or Escape); there are
no dialogs. A centred column at most 560 px wide: a title with Close on the right, then sections
(10 px `text.tertiary` heading over a 1 px `line`), each setting a 34 px row with its name (11 px
`text.primary`), a note under it (10 px `text.secondary`) and its control on the right, a
segmented control of 64 px per part. A change applies at once and is saved; one that needs a
restart (the renderer) says so in its note.

| Section | Setting | Values |
|---|---|---|
| Interface | Animations | On (default), Off |
| Arrangement | Track headers | Left, Right (default) |
| Graphics | Renderer | GPU (wgpu, default), OpenGL (glow) |
| Audio | Output | The device in use (read only) |

The app keeps them in `settings.txt` (`key = value` lines) in its app-data folder; unknown lines
are skipped with a warning in the status bar.

## Device panel

| Part | Spec |
|---|---|
| Panel | `surface.1`, panels 1 px apart, end of chain is empty `surface.0` |
| Header | 20 px, `surface.2`, 6 px padding: on switch, name |
| On switch | 9 × 9 square, 1 px border; on: filled `text.primary`; off: outline `text.tertiary`, name `text.tertiary` |
| Parameter | 44 px column: 26 px knob (r 11, pointer to r 10), value 10 px mono, label 10 px `text.secondary`; 14 px between columns; pointer `accent` when the device is on |

## Clip editor (MIDI)

| Part | Spec |
|---|---|
| Columns | Clip properties 196 px · tools 112 px · editor |
| Clip properties | 20 px title with 2 px clip-colour strip; 6 px padding and group gap; each pair = labels row (10 px `text.secondary`) and values row (15 px mono fields); Loop as a 15 px toggle |
| Tools | Transpose field, then a two-column grid of 18 px buttons (×2, ÷2, Fit scale, Invert, Humanize, Quantize, Reverse, Legato) |
| Toolbar | 20 px: tabs (current has a 1 px underline), divider, Fold, Scale (on = `#C9C4BB`), scale name, grid value right |
| Ruler | 16 px; bar numbers `#C9C4BB`, beats `text.tertiary`; bar lines `#5A5751`, beat lines `#34332F` |
| Clip loop | 8 px row, 4 px bar `surface.3`, 2 × 8 handles `text.primary` |
| Key column | 36 px; white keys `surface.2`, black keys `surface.0`; 2 px scale mark `#6E6A63`; C labels with octave (C3 = MIDI 60) |
| Note rows | Scale rows `#23221F`, others `#1A1917`; 1 px row lines `#2A2926`, under C `line.strong` |
| Note | Track colour; opacity 0.4 + 0.6 × velocity / 127; name in 9 px when ≥ 2 beats wide; 1 px gap to neighbours; selected = 1 px `text.primary` inset |
| Velocity lane | 72 px default (resizable); 36 px scale column (127, 64, 1); one 1 px stem per note with a 5 × 2 head, chord notes 4 px apart; selected stem `text.primary` |
| Velocity tools | 18 px: Randomize, Ramp, Deviation as small value fields |
