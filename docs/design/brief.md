---
description: Loopwerk's visual language: principles, what stays familiar and what is our own, layout decisions and design tokens
read_when: UI design, colours, tokens, typography, theme, accent, track colours, browser design, squint test, reference mockups
type: spec
status: draft
last_verified: 2026-10-12
covers:
  - crates/ui/src/theme.rs
---

# Design brief

How Loopwerk looks. Read with [ADR 0009](../decisions/0009-ui-toolkit-and-look.md): the workflow
is familiar to Ableton Live users, the look is Loopwerk's own. This brief is what a designer (or a
design tool) gets, together with the functional specs in `docs/spec/ui-*.md`. A screenshot of Live
may show a design tool where things are; the look always comes from this brief, never from the
screenshot (ADR 0009, decision 4).

Reference mockups (our own work, minimal v3):
[Session](reference/session-minimal-v3.png) and [Arrangement](reference/arrangement-minimal-v3.png).
They are the visual target; this file records the decisions behind them, and
[`components.md`](components.md) the measurements and states of every component. Where the text and an
image disagree, the text wins. These images may be given to design tools and agents. Checked
against the "Make our own" table below on 2026-10-12: every row holds. The menu bar still uses
the same top-level names as Live (File, Edit, Create, View, Options, Help); menu structure is a
method of operation and free to follow, but our own names (for example Insert, Preferences) are
an option for the UI.

## Character

Minimal, dense, calm. A tool for long sessions: the grid and the timeline dominate, everything
else stays quiet. Colour appears in one place, the tracks and clips; the rest is graphite.
Hierarchy comes from lightness steps, not from cards, shadows, gradients or rounded corners.

## Principles

From [research](#sources) and the first design rounds:

1. **Grey first.** Build hierarchy with lightness only, check it in greyscale, add colour last.
2. **Quiet the surroundings** instead of shouting the important thing.
3. **States differ in lightness, not hue alone** (playing is brighter, not just "accent").
4. **Two type sizes:** 11 px for UI text, 10 px for small values. No bold headings. 9 px is
   kept for tags beside a name, 13 px for a page title (components › Global rules).
5. **One window, no pop-ups.** Menus and pickers open in place.
6. **Progressive disclosure.** Controls appear when there is room or need (track height, toggled
   sections), never all at once.
7. **Every grid shows its lines.** Columns and rows of the Session grid, bars and beats, key rows.
8. **Not pure black.** The darkest surface is a warm near-black so the eye can adapt.

## Keep familiar (behaviour and structure)

- Window regions: control bar on top, browser on the left, the main view in the centre, the
  detail area (devices or clip editor) at the bottom, info text and status at the very bottom.
- Session view as a grid: tracks are columns, scenes are rows, scene launch on the right, return
  tracks and the main track after the regular tracks.
- Arrangement view: timeline left, track headers on the right with their controls.
- Interactions: click to launch, pending state while quantised, drag from the browser, double-click
  to open a clip, Tab between Session and Arrangement; keyboard shortcuts and menu structure.
- Number fields that are their own sliders (drag to change, the fill shows the value).

## Make our own (appearance)

| Area | Loopwerk | Avoid |
|---|---|---|
| Base palette | Warm graphite surfaces in four lightness steps (see tokens) | Live's neutral mid-grey |
| Accent | Iris `#A6A3FF`, used sparingly: transport play, solo, queued, device on, selected parameter | Orange as the active colour; cyan as the value colour |
| Track colours | Our own muted palette (terracotta, ochre, steel blue, lavender, …) | Live's saturated pastel palette |
| Type | Encode Sans Semi Expanded for UI text, IBM Plex Mono for numbers and time | Live's typeface |
| Knobs | Small circle with a pointer line, value below | Live's open arc knobs |
| Value fields | Neutral grey fill; volume fills full height with a level tick at the edges only; pan is a thin bottom bar growing from a centre tick | Live's cyan-filled boxes |
| Toggles | Track on = light grey number button; solo = accent; arm = red ring, filled when armed | The orange numbered track button |
| Clip slots | Flat cells, a small square in the track colour; playing = brighter cell with an accent progress line; queued = accent outline; recording = red dot and line | Fully coloured slots with triangle and square glyphs |
| Arrangement clips | Neutral body, 2 px top strip and note or wave marks in the track colour | Colour-filled clip bodies |
| MIDI notes | Track colour, velocity as brightness, selected note with a light outline | Velocity as saturation; blue selected notes |
| Device panels | Plain panels, a square on/off switch in the header | Live's title bar with a round activator |
| Meters | Two thin bars, green with yellow only near the top | Live's meters |
| Browser | Search, category filters as buttons, sidebar of places only (Pinned, Recent, Folders), tree with type icons, colour tags as dots on rows, preview with waveform, BPM and key | Live's sidebar of Collections, Library categories and Places |
| Icons | Lucide (ISC) or our own simple strokes | Any glyph traced from Live |
| Text | Our own wording ("Drop clips or devices to add a track", "All MIDI in") | Live's interface strings |
| Logo, name, splash | Entirely Loopwerk's own | Anything resembling Ableton or Push branding |

## Layout decisions

- **Grid lines:** Session columns are separated by `line.strong`, scene rows by `line`; the
  separators run through the mixer section. Arrangement tracks are separated by `line.strong`.
- **Track headers grow with track height** (Arrangement): compact = name and on/solo/arm; then
  volume and pan; then routing (output, and input, channel and monitor In/Auto/Off with
  recording); sends ("Send A", "Send B") go above routing once the engine plays them. Groups are
  separated by space only, routing gets a little extra space above it (components › Track header
  levels has the heights).
- **Detail area is resizable** with a splitter, up to nearly the full window height
  ([clip editor spec](../spec/ui-clip-editor.md)).
- **Clip properties panel:** one label row and one value row per pair, no group titles; Loop is a
  toggle row.

## Tokens

All colours, spacing, radii, stroke widths and type sizes are named tokens in one theme module.
Widgets never contain literal colours. Current values:

| Token | Value | Use |
|---|---|---|
| `surface.0` | `#1D1C1A` | Grid background, empty slots, lanes |
| `surface.1` | `#252421` | Panels, headers, browser |
| `surface.2` | `#2E2D2A` | Fields, stopped clips, buttons |
| `surface.3` | `#45433F` | Selected, pressed |
| `surface.3.bright` | `#4A4843` | Playing clip, playing scene, volume fill |
| `line` | `#383733` | Separators between regions and rows |
| `line.strong` | `#45433F` | Session columns, arrangement tracks |
| `text.primary` | `#ECE8E1` | |
| `text.secondary` | `#9A958C` | |
| `text.tertiary` | `#66625B` | Labels, inactive |
| `accent` | `#A6A3FF` | |
| `state.recording` | `#E5484D` | |
| `meter.ok` / `meter.hot` | `#8BC48A` / `#E3C86A` | |
| `track.1`–`track.4` | `#D08F6E` `#C6AC5E` `#73AAC0` `#A497CC` | (palette to extend to 16) |
| type | 11 px UI, 10 px values, 9 px rulers | |
| row height | 20 px grid rows and fields, 15–16 px inline fields, 17 px browser rows | |
| radius | 0 everywhere | |

## Squint test

Before a view ships, put it next to the same view in Live:

1. An Ableton user can operate it without help. **Familiar enough.**
2. At thumbnail size, nobody mistakes it for Live. **Different enough.**

If (2) fails, change colours, control shapes, typography and wording, not the layout.

## Sources

- Robert Henke on one window and no pop-ups, via
  [linux-audio-user 2003](https://ccrma.stanford.edu/mirrors/lalists/lau/2003/01/0328.html).
- [How we redesigned the Linear UI](https://linear.app/now/how-we-redesigned-the-linear-ui):
  LCH-based theme from base, accent and contrast; limited accent use.
- [Ardour: meter contrast](https://tracker.ardour.org/view.php?id=6529),
  [linux-audio-dev on dark themes](https://lists.linuxaudio.org/archives/linux-audio-dev/2022-June/038377.html),
  [MOD forum on knob readability](https://forum.mod.audio/t/plugin-artwork/7353/98).
- [Mycroft, QMUL: visual load and listening](https://qmro.qmul.ac.uk/xmlui/handle/123456789/44047?show=full).
