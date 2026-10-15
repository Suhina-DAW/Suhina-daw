---
description: How launching clips and scenes behaves: quantisation, stop buttons, scene tempo, launch modes, legato, per-clip quantisation, launcher over arrangement
read_when: clip launching, scenes, launch quantisation, stop, session view, launcher commands, launch modes, gate, toggle, repeat, legato, per-clip quantisation
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/engine/src/launcher.rs
  - crates/engine/src/launcher/modes.rs
  - crates/model/src/launch.rs
  - crates/session/src/launch.rs
  - crates/session/tests/launcher.rs
  - crates/session/tests/launch_settings.rs
---

# Clip launching

- **Sources:** the behaviour of the Session View as the public Live 12 manual describes it
  (launching clips and scenes, launch quantisation, stop buttons, scene tempo, clip launch
  modes, legato and clip quantisation), and the Live Set
  fields in [`als-format.md`](als-format.md) (scenes, clip slots, `HasStop`, scene tempo, clip
  markers; corpus series 1 and 4).
- **Observed with:** nothing yet. The launch behaviour has not been checked against Live 12.4.6
  by listening or watching; every statement marked **(hypothesis)** needs that check (see
  *To observe* below). The Live Set fields come from the corpus and are observed.
- **Status:** implemented in `crates/engine/src/launcher.rs` (ROADMAP M4); launch modes, legato
  and per-clip quantisation in `crates/engine/src/launcher/modes.rs`. Follow actions are not
  specified yet. Reading the launch settings from a Live Set needs a corpus series of its own.

This document describes the behaviour Suhina implements, in Suhina's terms. Where Suhina
decides something the sources do not settle, the text says so (**Suhina:**).

## Terms

- **Launcher:** a grid of **columns** (one per track) and **scenes** (rows). Each cell is a
  **slot**: empty, or holding one clip. An empty slot may have a **stop button**.
- **Transport:** the clock that clips are launched against, counted in beats (quarter notes)
  from the moment it starts. A bar is `numerator × 4 / denominator` beats.
- **Launch quantisation:** the grid that launches wait for. Choices: none, 8, 4, 2 or 1 bars,
  or a note value (1/2, 1/4, 1/8, 1/16, 1/32, and the triplets 1/2T, 1/4T, 1/8T, 1/16T).
  Default: 1 bar.

## Behaviour

1. **One clip per column.** A column plays at most one clip. Launching a clip in a column replaces
   the clip that column was playing, at the launch moment.
2. **Quantised launch.** A launch (clip, scene or stop) waits for the next quantisation boundary
   of the transport. A launch on a boundary happens at once. With quantisation off it happens at
   once. Until then the column shows the launch as queued; a later launch in the same column
   replaces the queued one.
3. **Launching starts the transport.** If the transport is stopped, launching a clip or scene
   starts it at the play start marker (beat 0 unless moved,
   [arrangement editing](arrangement-editing.md) › Play position) and the launch happens
   immediately. **(hypothesis)**
4. **Playing a clip.** A launched clip plays from its start marker. With its loop on, it plays to
   the loop end, then repeats the loop region until it is stopped or replaced. With its loop off,
   it plays from the start marker to the end marker once and the column stops.
5. **Scenes.** Launching a scene launches, in every column, the slot in that scene's row:
   a clip is launched; an empty slot with a stop button stops the column; an empty slot without
   one leaves the column as it is. If the scene has a tempo, the project tempo changes to it at
   the launch moment. **(hypothesis:** at the quantised launch moment, not when the scene is
   triggered.**)**
6. **Stopping.** Stopping a column (its stop button, or an empty slot with a stop button) is
   quantised like a launch. "Stop all clips" stops every column, quantised. Stopping the
   transport stops everything at once, and clears queued launches. A jump of the playhead
   (`transport.setPosition` while playing) keeps queued launches, queued again for the next
   boundary from the new beat.
7. **Deactivated clips** do not play. **Suhina:** launching one is ignored, alone or in a
   scene: its column keeps what it plays (an empty slot's stop button would stop it).
8. **Launcher over arrangement.** A track whose column has launched a clip no longer plays its
   arrangement clips, even after that column stops, until "back to arrangement" returns every
   track to the arrangement. **(hypothesis** for the "even after that column stops" part.**)**
   **Suhina:** back to arrangement stops the launched clips of those tracks (with the stop
   fade) and drops their queued launches, so a track never plays both at once.

## Launch settings

Every clip has launch settings. They count when the clip is in a slot; a clip on the arrangement
keeps them (so it launches the same way when it is put into a slot again) but does not use them.
A new clip has the defaults: trigger, legato off, global quantisation.

A slot's launch button has a **press** and a **release**. A mouse button going down on the slot
is the press and going up is the release; `clip.launch` is the press and `clip.release` the
release for the CLI and agents. A click that has no press and release of its own (the keyboard,
an accessibility tool) is a press.

1. **Launch mode.**
   - **Trigger** (default): the press launches the clip; it plays until it is stopped or
     replaced. The release does nothing.
   - **Gate:** the press launches the clip, the release stops it, at the clip's quantisation
     like any stop. **Suhina:** a release before the launch has happened takes the launch back,
     and the clip does not play. **(hypothesis:** the stop at the release is quantised.**)**
   - **Toggle:** the press launches the clip; a press while it plays stops it, quantised.
     **Suhina:** a press before a queued launch or stop of this clip has happened takes that
     launch or stop back.
   - **Repeat:** the press launches the clip, and while the button is held it is launched again
     at every boundary of its quantisation. **Suhina:** with no quantisation there is no boundary
     to repeat on, and the clip plays as if triggered. After the release the clip plays on from
     where it is **(hypothesis)**; only the next repeat is taken back.
2. **Legato.** A clip with legato on starts at the position of the clip it replaces on its track,
   instead of at its own start. **Suhina:** the position is the time since the replaced clip was
   launched, which carries on from clip to clip while one legato clip replaces another; a clip
   that loops wraps it into its loop, one that does not wraps it into its length. On a track that
   plays nothing the clip starts from its start. Notes that start before that position do not
   sound; audio plays from that place in the file.
3. **Clip quantisation.** A clip's launch waits for its own grid instead of the global launch
   quantisation: **global** (default; the launcher's value, followed as it changes), **none**,
   or any of the global values. The stop of a gate or toggle clip waits for the same grid.
   **Suhina:** a scene launch launches each clip on its own grid; the track's own stop button,
   "stop all clips" and a scene's tempo keep the global grid.
4. **Scenes.** A clip launched by a scene starts like a triggered clip, whatever its mode: a
   scene has no release, and launching it again relaunches its row. Legato and the clip's
   quantisation apply. **(hypothesis)**
5. **Another launch in the column.** A held gate or repeat clip that another launch replaces
   (another slot, or a scene) no longer listens for its release.

**Commands.** `clip.setLaunchMode {track, scene, mode}` (trigger, gate, toggle, repeat),
`clip.setLegato {track, scene, on}` and `clip.setQuantization {track, scene, value}` (global,
none, 8 bars … 1/32) change the clip in a slot, one undo step each; an edit while the clip is
held or playing is heard at once ([ADR 0005](../decisions/0005-edits-while-playing.md)).
`clip.release {track, scene}` is a transport command like `clip.launch`. `project.launcher`
reports each clip's `launch_mode`, `legato` and `quantization`. The project file keeps the
settings ([Suhina format](suhina-format.md)).

## Suhina decisions

- **Notes cut at the loop end.** A note that would sound past the loop end (or past the end
  marker of a clip without a loop) is shortened to end there.
- **Instruments.** A launched clip's notes play on the track's instrument when its device
  chain starts with one, like arrangement notes; a launch that replaces a clip, stopping the
  column or stopping the transport releases them (ADR 0004).
- **Fades.** A stopped or replaced audio clip fades out over 5 ms instead of cutting, and every
  launched audio clip fades in over 2 ms, to avoid clicks; one that does not loop also fades out
  over 2 ms at its end marker. The fade-in and fade-out an
  arrangement clip has ([arrangement editing](arrangement-editing.md) › Fades) do not apply to
  launched clips yet.
- **Time-stretching.** Launched warped clips are time-stretched like arrangement clips (ADR
  0003): each one's stretcher is prepared for the project tempo when the project opens and
  rewound at every launch.
- **Tempo changes** from a scene affect the launcher clock. Warped audio clips follow the new
  tempo by reading faster or slower (re-pitch), since their stretchers are prepared for the
  project tempo; unwarped audio clips keep playing at the speed of their file. The arrangement
  keeps its frame positions for now; a tempo map for the arrangement comes later.
- **Commands.** Every action is a command (`clip.launch`, `clip.release`, `scene.launch`, `track.stop`,
  `launcher.stopAll`, `launcher.setQuantization`, `launcher.backToArrangement`,
  `transport.play`, `transport.stop`, `transport.setPosition`, `transport.setMetronome`,
  `launcher.state`), sent to the audio thread through a lock-free queue and applied at the start
  of the next audio block. **Suhina:**
  `launcher.state` also reports the browser's preview (`preview`: playing, seconds), which plays
  beside the launcher and never changes it ([ADR 0011](../decisions/0011-preview-and-added-audio.md)),
  and the engine's CPU load (`cpu`: percent), which the control bar shows. `transport.stop`
  with `return: "here"` stops and keeps the position where the transport stopped
  ([arrangement editing](arrangement-editing.md) › Play position).

## To observe

Checks in Live 12.4.6 that would turn the hypotheses above into observations (a corpus series
with a screen recording or notes, no audio from Live needed):

- With the transport stopped, launch a clip: does it start at once, and does the bar count
  start from it?
- Launch a scene with a tempo while another scene plays: does the tempo change when the scene is
  triggered or at the quantised launch?
- Launch a clip on a track that has arrangement clips, then stop it: does the track stay silent
  until "Back to Arrangement"?
- Global quantisation: where it is stored in the Live Set, and its default in a new set.
- Gate: is the stop at the release quantised, and does a release before the launch keep the
  clip from playing?
- Repeat: after the release, does the clip play on or stop?
- A scene launch: do the clips in its row keep their launch modes, legato and quantisation?
- Where a clip's launch mode, legato and quantisation are stored in a Live Set (a corpus series
  that changes one of them per saved variant).

## Not yet

- Follow actions.
- Reading the global launch quantisation and the clips' launch settings from a Live Set (needs a
  corpus series).
- Launching slots from computer keys and MIDI notes (their press and release would drive gate
  and repeat the same way).
- Launch velocity.
- Recording into slots.
