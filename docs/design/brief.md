# Design brief

How Loopwerk looks. Read with [ADR 0002](../decisions/0002-ui-toolkit-and-look.md): the workflow
is familiar to Ableton Live users, the look is Loopwerk's own. This brief is what a designer (or a
design tool) gets, together with the functional specs in `docs/spec/ui-*.md`. Never give a design
tool screenshots of Live.

## Character

Fast, precise, calm. A tool for long sessions on Linux desktops: dark by default, high contrast
for the things that matter (what is playing, what is selected, what is recording), quiet for
everything else. Dense but never cramped. Every state readable at a glance from across the room.

## Keep familiar (behaviour and structure)

- Window regions and where they sit: control bar on top, browser on the left, the main view in
  the centre, detail area (devices or clip editor) at the bottom, info text and status at the
  very bottom.
- Session view as a grid: tracks are columns, scenes are rows, scene launch on the right, return
  tracks and the main track after the regular tracks.
- Per-track mixer and routing directly below the grid; device chains run left to right.
- Interactions: click to launch, quantised start shown as a pending state, drag from the browser
  onto tracks or slots, double-click to open a clip, Tab between Session and Arrangement.
- Keyboard shortcuts and menu structure.

## Make our own (appearance)

| Area | Loopwerk direction | Avoid |
|---|---|---|
| Base palette | Deep blue-black surfaces with clear elevation steps | Live's neutral mid-grey flat surfaces |
| Accent | One signature accent colour (to choose) used for "active" and "playing" | Orange as the active/enabled colour |
| Track colours | Our own palette, tuned for the dark base | Live's pastel clip colour palette |
| Type | Inter for UI text, JetBrains Mono for numbers and time | Live's typeface and its letter-spacing |
| Knobs | A distinct control: e.g. a filled ring with the value inside, or compact horizontal sliders | Thin open arc knobs with a pointer line |
| Toggles | Labelled or icon toggles with clear on/off shape (filled vs outline) | Small square buttons that turn orange; the numbered track-on button |
| Clip slots | Rounded cells with a colour bar and a progress ring or bar while playing | Flat rectangles with a triangle play glyph on the left and a square stop glyph |
| Device panels | Cards with a header row and a visible on/off switch in our style | Live's device title bar with a round activator on the left |
| Meters | Our own meter style (e.g. segmented with a peak line) | Live's thin green meters |
| Icons | Lucide (ISC), plus our own drawings | Any glyph traced from Live |
| Text | Our own wording for hints, empty states and help ("Drop clips here", etc. rewritten) | Live's interface strings copied verbatim |
| Logo, name, splash | Entirely Loopwerk's own | Anything resembling Ableton or Push branding |

## Tokens

All colours, spacing, radii, stroke widths and type sizes are named tokens in one theme module.
Widgets never contain literal colours. Start with: `surface.0`–`surface.3`, `text.primary`,
`text.secondary`, `accent`, `accent.muted`, `state.playing`, `state.queued`, `state.recording`,
`state.muted`, `track.1`–`track.16`, `space.1`–`space.6`, `radius.small`, `radius.large`.

## Squint test

Before a view ships, put it next to the same view in Live:

1. An Ableton user can operate it without help. **Familiar enough.**
2. At thumbnail size, nobody mistakes it for Live. **Different enough.**

If (2) fails, change colours, control shapes and typography, not the layout.
