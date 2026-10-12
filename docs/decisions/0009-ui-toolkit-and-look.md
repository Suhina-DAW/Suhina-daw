---
description: Why the UI is built with egui, and how close it may get to Live: same structure and behaviour, Loopwerk's own look
read_when: UI toolkit, egui, look and feel, trade dress, design rights, screenshots, how close to Ableton
type: decision
status: current
last_verified: 2026-10-12
---

# 0009: egui for the UI; familiar workflow, Loopwerk's own look

- **Status:** accepted
- **Date:** 2026-10-11

## Context

Loopwerk's UI has two hard parts. Most of it is custom drawing: the clip grid, the timeline,
waveforms, the piano roll, meters and device panels. And it must feel immediately familiar to
people coming from Ableton Live, while not looking like Live.

Toolkits considered:

| Toolkit | For | Against |
|---|---|---|
| **egui** (eframe, wgpu) | Custom drawing is direct; PhotoCraft shows a large professional app built on it; fast iteration; offscreen snapshots for tests and agents; builds for wasm | Immediate mode repaints every frame, so heavy views must cache their geometry |
| iced | Elm architecture, retained mode | Heavy custom canvases are more work; fewer professional apps to learn from |
| Slint | Declarative, efficient | Own markup language; custom drawing views are less direct |
| vizia | Built for audio (Meadowlark) | Less mature; Meadowlark is archived |

What the law protects decides how close the UI may get (summary, not legal advice):

- **Not protected:** functionality, workflow, the arrangement of views, keyboard shortcuts and
  menu structure as a method of operation (CJEU *SAS Institute v WPL*, 2012; US *Lotus v Borland*,
  1995).
- **Protected:** names and logos (trademark); icons, artwork and fonts (copyright); and the
  **appearance** of a user interface: in the EU a GUI can be a registered design, and an
  unregistered design is protected for three years after it is first shown.

## Decision

1. **The UI is built with egui** on eframe and wgpu.
2. **Same map, different paint.** The structure and behaviour of each view follow Live as closely
   as useful: where things are, what they do, how they react, which keys do what. The appearance
   is Loopwerk's own: colours, typography, control shapes, icons, spacing and the visual language
   of states. [`docs/design/brief.md`](../design/brief.md) lists what stays familiar and what
   must differ.
3. **Behaviour is specified in words.** Each view has a functional spec under `docs/spec/ui-*.md`
   written from using Live: regions, order, interactions, states, shortcuts. Specs contain no
   colours, sizes or images from Live.
4. **Screenshots are a reference for structure, never for the look** (amended 2026-10-12).
   Screenshots of Live may be used, also with a design tool, to see where things are and how
   they behave, as they are for the functional specs. The look is drawn from the design brief:
   its "Make our own" table decides colours, type, control shapes and states, whatever a
   screenshot shows. Screenshots are never traced or committed, and their icons, artwork and
   text are never reused. (The first version of this decision banned screenshots in design
   work altogether; the first design rounds had started from one, and the look was reworked
   into our own afterwards. What protects the project is the look being our own, so the rule
   now says that.)
5. **The UI is a thin client of the engine.** Every action is an engine command (AGENTS.md rule 6).
   The UI reads playback position, meters and clip states from snapshots the engine publishes
   and never touches the audio thread. UI state (zoom, selection, open panels) is serialisable,
   so agents can read it and take offscreen screenshots.
6. **Design tokens** (colours, spacing, radii, type sizes) live in one theme module, never
   hard-coded in widgets, so themes are possible later.

## Consequences

- Ableton users can work without relearning: the views are where they expect and react the way
  they expect.
- Nobody should mistake a Loopwerk screenshot for Live, even at thumbnail size. Every view is
  checked side by side before release (the brief's "squint test").
- Heavy views (clip grid with many clips, arrangement, waveforms) need geometry caches keyed on
  project revision; this is part of the UI architecture from the start.
- UI implementation starts after M3 (commands); specs and the design brief can be written now.
