---
description: Why tracks and mixer channels are separate and clips place shared contents, so several DAWs' projects fit
read_when: changing the model, track, channel, clip content, lane, pattern, importing another format
type: decision
status: current
last_verified: 2026-10-12
covers:
  - crates/model/src/lib.rs
---

# 0001: A DAW-neutral project model

- **Status:** accepted
- **Date:** 2026-10-11

## Context

Suhina should open projects from more than one DAW: Ableton Live (`.als`) first, later
DAWproject and FL Studio (`.flp`), and possibly others. These programs organise a song in
different ways:

| Concept | Ableton Live | FL Studio | DAWproject |
|---|---|---|---|
| Where notes and audio live | A clip belongs to one track | A pattern holds notes for many channels at once | `Clip` with inline `content`, or a `reference` to shared content |
| Reuse of the same content | Clips are copied; each copy is independent | One pattern is placed many times in the playlist; editing it changes every instance | `Clip.reference` points at one shared timeline |
| Timeline rows | Each arrangement row is a track with its own mixer strip | Playlist tracks are free rows, not tied to a channel or mixer insert | `Lanes`, each optionally bound to a `Track` |
| Instrument vs mixer | Track = instrument/effects chain + mixer strip | Channel rack (generators) routes to separate mixer inserts | `Track` (structure) and `Channel` (mixer strip) are separate |
| Clip launching | Session view: grid of slots × scenes | None (performance mode aside) | `Scene` + `ClipSlot` |

The FL Studio column is from general knowledge and will be verified and documented in
`docs/spec/` before any `.flp` work starts (sources: observation, self-authored files and the
GPL-3.0 PyFLP project).

If the model were shaped like `.als`, an FL pattern that drives eight channels would have to be
split into eight unrelated clips on import, and the link between its instances would be lost.

DAWproject ([bitwig/dawproject](https://github.com/bitwig/dawproject), MIT, version 1.0) was
designed as a vendor-neutral interchange format by Bitwig and PreSonus and already solves this.
Its key ideas:

- **Track and Channel are separate.** A track is a node in the project structure; a channel is a
  mixer strip with volume, pan, mute, sends, devices and an output destination.
- **Content is separate from placement.** A `Clip` places a timeline at a time, with play range,
  loop range and fades. Its content is either inline or a `reference` to a shared timeline.
- **A timeline can target a track.** `Lanes` contain timelines, each optionally bound to a track,
  so one piece of content can carry notes for several tracks.
- **Time units are explicit.** Every timeline says whether it counts in beats or seconds; audio
  is mapped to musical time with warp markers (`time` ↔ `contentTime`).
- **Plug-in state is opaque.** Devices store their state as a blob that is handed back to the
  plug-in unchanged.

## Decision

`suhina-model` is a DAW-neutral model built on DAWproject's ideas, in three layers:

1. **A common core** of concepts every DAW has:
   - a transport (tempo, time signature)
   - **tracks** (structure) and **channels** (mixer strips: volume, pan, mute, solo, sends,
     output, device chain) as separate objects
   - a pool of **clip contents**, each made of **content lanes**; a lane holds notes or audio and
     may target a specific track
   - **clips** that place a content from the pool on the arrangement or in a launcher slot, with
     their own start, length, offset and loop range, and (audio on the arrangement) a fade-in and
     a fade-out in beats ([arrangement editing](../spec/arrangement-editing.md) › Fades)
   - an **arrangement** of lanes, each optionally bound to a track
   - a **launcher**: scenes and per-track slot columns
2. **Per-format extensions.** Every object carries an `Extensions` list where an importer keeps
   data the core does not model, keyed by format, verbatim. The exporter for the same format
   writes it back, so `.als` → Suhina → `.als` loses nothing the core does not understand yet.
3. **Formats live in their own crates** (`crates/als`, later `crates/flp`, `crates/dawproject`) and
   convert to and from the core. Adding a format never requires changing another format.

Objects refer to each other by typed ids (`TrackId`, `ChannelId`, `ContentId`), not by pointers.
`Project::validate` checks that every id resolves and that every arrangement clip's fades fit it,
so a malformed import becomes an error instead of a crash.

How each DAW maps onto the core:

- **Ableton clip** → a clip content with one lane that has no target (it plays on the track it
  is placed on), placed by one clip. Copies of a clip get their own content.
- **FL pattern** → one clip content whose lanes each target a channel's track; every playlist
  instance is a clip pointing at the same content.
- **FL playlist track** → an arrangement lane with no track binding.
- **Ableton Session view** → launcher columns (one per track) × scenes.

## Amendment: built-in device parameters (2026-10-11)

A `Device` also holds `params`: the parameter values of a built-in device
(`DeviceKind::Builtin`), as a list of a stable parameter id and a value in the parameter's own
unit (decibels, hertz, a fraction, an option index). Parameters that are not listed keep their
defaults, and the device limits each value to its range when it loads it, so a project written
by an older or newer Suhina still opens. Plug-ins keep using `state`; placeholders have
neither. Importers fill `params` when they map another DAW's device onto a Suhina device
(first: Live's Utility, `docs/spec/als-format.md` › Devices).

## Consequences

- Importers do real conversion work instead of copying a file's structure, and every importer
  needs tests that show nothing is lost on a round trip.
- Shared content means editing a clip content can change several clips at once. Commands must
  offer "make unique" (copy-on-edit) for DAWs that expect independent copies.
- The engine plays the core model only; it never sees format-specific data.
- Plug-in state stays opaque, but in each plug-in format's standard container where a source
  format keeps its own: a VST2 plug-in's state is the standard VST2 bank (`.fxb`), which other
  hosts and a VST3 version of the plug-in read (ADR 0006, decision 8).
- The core will grow (automation, tempo maps, markers, groups, comping). Each addition is checked
  against at least two formats before it lands, so it does not bake in one DAW's assumptions.
- Positions are `f64` beats in the core. If rounding errors appear against tick-based formats
  (FL uses PPQ ticks), we revisit this with a rational or tick-based time type.
