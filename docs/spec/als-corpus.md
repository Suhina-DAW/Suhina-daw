---
description: How to author the Live Sets used to learn the .als format and test the importer, series by series
read_when: making test sets, corpus, new series, what to save in Live, test files
type: howto
status: current
last_verified: 2026-10-11
covers:
  - crates/als/tests/corpus.rs
---

# Making the `.als` test corpus

How to author the Live Sets we use to learn the `.als` format and to test the importer. Read
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room first: everything in the corpus must be
made by you, in your own copy of Live, from scratch.

## Rules

- **Change one thing per file.** Each file is the previous one plus exactly one edit, saved under
  a new name. Then a diff between neighbours shows only what that edit changed.
- **Only your own content.** No samples, presets, clips or devices from Live's Core Library or
  Packs. Audio comes from your own recording or from Loopwerk itself (see step 08).
- **Don't click around.** Live also saves view state (selected track, scroll position, open
  panels). Every extra click adds noise to the diff. Make the edit, save, move on.
- **Record what you did.** Fill in the log table at the end (copy it to `corpus/als/README.md`).

## Setup

1. Note your Live version and edition (Help › About Live, e.g. "Live 12.2.5 Suite") and your OS.
2. Create the folder `corpus/als/` in your Loopwerk checkout. It is gitignored; nothing here is
   committed.
3. Start from a new Live Set (File › New Live Set).

Use **File › Save Live Set As…** for every step and save into `corpus/als/`. Live creates a
project folder the first time; keep saving all sets into that same project folder, so they
share one `Samples/` folder.

## Series 1: basics

| # | File name | Edit from the previous file |
|---|---|---|
| 00 | `00-base.als` | Delete every track you can (select all tracks, Delete). Keep the master. Tempo 120, 4/4. Save. |
| 00b | `00b-base-resaved.als` | **No edit.** Open `00-base.als` again and save it under the new name. This shows which fields change on every save (ids, timestamps) and are noise. |
| 01 | `01-tempo-128.als` | From `00-base`: set the tempo to 128. |
| 02 | `02-midi-track.als` | From `00-base`: add one MIDI track (Create › Insert MIDI Track). No instrument. |
| 03 | `03-midi-clip-empty.als` | Double-click the first Session slot of the MIDI track to create an empty clip (default length, one bar). |
| 04 | `04-midi-clip-one-note.als` | In that clip, draw one note: C3, on beat 1, length 1/4, default velocity. |
| 05 | `05-note-moved.als` | Move the note to beat 2. Nothing else. |
| 06 | `06-note-velocity.als` | Change the note's velocity to 64. |
| 07 | `07-note-key.als` | Change the note to E3. |
| 08 | `08-clip-length-2-bars.als` | Set the clip length to 2 bars (Clip View, loop length). |
| 09 | `09-clip-loop-off.als` | Turn the clip's Loop switch off. |
| 10 | `10-clip-name-color.als` | Rename the clip to `Test` and pick a different clip colour. |
| 11 | `11-second-scene.als` | Add a second scene (Create › Insert Scene). |
| 12 | `12-second-clip.als` | Create a clip in scene 2 of the MIDI track with one note: G3 on beat 1. |
| 13 | `13-clip-in-arrangement.als` | Switch to Arrangement View and drag the scene-1 clip to bar 1 of the MIDI track. |
| 14 | `14-arrangement-moved.als` | Move that arrangement clip to bar 3. |
| 15 | `15-track-volume-pan.als` | Set the MIDI track's volume to -6 dB and pan to 50L. |
| 16 | `16-track-mute.als` | Mute the MIDI track (Track Activator off). |
| 17 | `17-return-send.als` | Unmute. Turn the MIDI track's Send A up to -12 dB. |

## Series 2: audio

Make the audio file with Loopwerk so its origin is clean:

```sh
cargo run -p loopwerk-cli -- render --demo -o corpus/als/loopwerk-demo.wav
```

Continue from the last file of series 1 and use an audio track that has no clips yet.

| # | File name | Edit from the previous file |
|---|---|---|
| 30 | `30-audio-clip.als` | In Session View, drag `loopwerk-demo.wav` from your file manager into the track's first clip slot. Leave warp settings as Live sets them. |
| 31 | `31-audio-warp-off.als` | Open the clip (Clip View) and turn Warp off. |
| 32 | `32-audio-warp-marker.als` | Turn Warp on again, add one warp marker on beat 2 and drag it a little to the right. |
| 33 | `33-audio-gain.als` | Set the **clip's** Gain (Clip View, not the mixer) to -6 dB. |
| 34 | `34-audio-transpose.als` | Set the clip's Transpose to +3 st. |
| 35 | `35-audio-in-arrangement.als` | In Arrangement View, drag `loopwerk-demo.wav` to bar 1 of the same track. |

## Series 3: devices

| # | File name | Edit from the previous file |
|---|---|---|
| 40 | `40-midi-track-instrument.als` | Drag Live's built-in **Simpler** (empty, no sample) onto a MIDI track. Not a preset. |
| 41 | `41-audio-effect.als` | Add a built-in **EQ Eight** after it, default settings. |
| 42 | `42-device-param.als` | Change one EQ Eight parameter (band 1 gain to +3 dB). |
| 43 | `43-device-off.als` | Turn EQ Eight off with its device activator. |

If you have a CLAP or VST3 plug-in with a free licence, also save `44-plugin.als` with it on the
track and note the plug-in's name and version.

## Series 4: open questions

Answers the open questions in [`als-format.md`](als-format.md). Start from
`36-audio-clip-gain.als`, not from a `44-*` file, so no commercial plug-in state ends up in these
sets. "The C3 clip" is the session clip in 1-MIDI slot 1 (C3, loop on, 2 bars).

| # | File name | Edit from the previous file |
|---|---|---|
| 50 | `50-time-signature-3-4.als` | In the transport bar, set the time signature to 3/4 (type `3` in the numerator). |
| 51 | `51-time-signature-6-8.als` | Set 6/8. |
| 52 | `52-time-signature-7-8.als` | Set 7/8. |
| 53 | `53-time-signature-4-4.als` | Back to 4/4. |
| 54 | `54-clip-start-marker.als` | Clip View of the C3 clip: type `1.2.1` in **Start**. |
| 55 | `55-clip-loop-position.als` | Type `1.3.1` in the loop **Position**. |
| 56 | `56-clip-loop-off.als` | Turn **Loop** off. |
| 57 | `57-clip-end-marker.als` | Type `2.3.1` in **End**. |
| 58 | `58-clip-deactivated.als` | Select the C3 clip in Session View and press `0` (deactivate clip). |
| 59 | `59-note-deactivated.als` | Open the G3 clip (1-MIDI slot 2), select the note and press `0` (deactivate note). |
| 60 | `60-group-track.als` | Select the headers of 3-Audio and 4-loopwerk-demo (Ctrl-click) and press `Ctrl+G`. |
| 61 | `61-audio-warp-off.als` | Session audio clip on 4-loopwerk-demo: Warp off. |
| 62 | `62-audio-unwarped-loop-start.als` | Move the loop **Position** to about one second. Log the exact value Live shows. |

## Tips from the first run

- Live's default new set already has two MIDI tracks, two audio tracks, two returns and eight
  scenes. Edits that "add" one of these may not be needed; check the log of what was really done.
- Clip View controls only appear for the clip you double-clicked. Mixer controls in the track
  column change the **track**, not the clip.
- Typing values into fields (`-6`, `50L`, `2`) gives exact numbers; dragging gives values like
  `64.251297`.
- Saving before or after closing a panel moves view-state changes between files. Harmless, but
  log it.

## Extract and diff

Live puts the sets in the project folder it created on the first save (`00-base Project/`):

```sh
scripts/als-extract.sh corpus/als/
P="corpus/als/00-base Project"
git diff --no-index "$P/00-base.als.xml" "$P/00b-base-resaved.als.xml"
git diff --no-index "$P/00-base.als.xml" "$P/01-tempo-128.als.xml"
```

`code --diff a.xml b.xml` gives a side-by-side view in VS Code.

## Log

Copy this table to `corpus/als/README.md` and fill it in as you go.

```markdown
Live version: Live 12.x.x <edition>
OS:
Author:
Date:

| File | Done | Notes (anything unexpected, extra clicks, dialogs) |
|---|---|---|
| 00-base.als | | |
| 00b-base-resaved.als | | |
| 01-tempo-128.als | | |
```

## Later series

Not yet; each gets its own section when the importer needs it: tempo and time-signature
automation, track automation, groups, return tracks, follow actions, launch quantisation and
launch modes, MIDI CC and pitch-bend envelopes, comping and take lanes, Max for Live devices
(as opaque placeholders), and sets saved by older Live versions.
