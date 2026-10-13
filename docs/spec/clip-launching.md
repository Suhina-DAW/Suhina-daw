---
description: How launching clips and scenes behaves: quantisation, stop buttons, scene tempo, launched clip fades, launcher over arrangement
read_when: clip launching, scenes, launch quantisation, stop, session view, launcher commands, fades on launched clips
type: spec
status: current
last_verified: 2026-10-13
covers:
  - crates/engine/src/launcher.rs
  - crates/session/tests/launcher.rs
  - crates/engine/src/fade_tests.rs
---

# Clip launching

- **Sources:** the behaviour of the Session View as the public Live 12 manual describes it
  (launching clips and scenes, launch quantisation, stop buttons, scene tempo), and the Live Set
  fields in [`als-format.md`](als-format.md) (scenes, clip slots, `HasStop`, scene tempo, clip
  markers; corpus series 1 and 4).
- **Observed with:** nothing yet. The launch behaviour has not been checked against Live 12.4.6
  by listening or watching; every statement marked **(hypothesis)** needs that check (see
  *To observe* below). The Live Set fields come from the corpus and are observed.
- **Status:** first version, implemented in `crates/engine/src/launcher.rs` (ROADMAP M4).
  Follow actions, per-clip launch quantisation and launch modes (trigger, gate, toggle, repeat)
  are not specified yet; they need a corpus series of their own.

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

## Suhina decisions

- **Notes cut at the loop end.** A note that would sound past the loop end (or past the end
  marker of a clip without a loop) is shortened to end there.
- **Instruments.** A launched clip's notes play on the track's instrument when its device
  chain starts with one, like arrangement notes; a launch that replaces a clip, stopping the
  column or stopping the transport releases them (ADR 0004).
- **Fades.** A stopped or replaced audio clip fades out over 5 ms instead of cutting, and every
  launched audio clip fades in over 2 ms, to avoid clicks; one that does not loop also fades out
  over 2 ms at its end marker. These click guards stay under the clip's own fades below.
- **Clip fades.** A clip in a slot has the same fade-in and fade-out as an arrangement clip
  (lengths in beats, shapes and curves; [arrangement editing](arrangement-editing.md) › Fades),
  fitted to the clip's own length. The **fade-in** plays once, from the launch: a looping clip
  does not fade in again at each loop pass, since a fade at every pass would make the loop pulse.
  The **fade-out** plays where the clip ends by itself, at the end marker of a clip without a
  loop, with the same envelope an arrangement clip of that length has. A looping clip never
  ends by itself, so its fade-out does not sound. Stopping or replacing a launched clip keeps the
  5 ms stop fade rather than the clip's fade-out, so the stop stays on its quantisation
  boundary. `clip.setFades {track, scene, …}` sets a slot clip's fades, with the same parameters
  and checks as for an arrangement clip (`scene` instead of `index`); one undo step.
- **Time-stretching.** Launched warped clips are time-stretched like arrangement clips (ADR
  0003): each one's stretcher is prepared for the project tempo when the project opens and
  rewound at every launch.
- **Tempo changes** from a scene affect the launcher clock. Warped audio clips follow the new
  tempo by reading faster or slower (re-pitch), since their stretchers are prepared for the
  project tempo; unwarped audio clips keep playing at the speed of their file. The arrangement
  keeps its frame positions for now; a tempo map for the arrangement comes later.
- **Commands.** Every action is a command (`clip.launch`, `scene.launch`, `track.stop`,
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

## Not yet

- Follow actions, launch modes, per-clip launch quantisation, legato.
- Reading the global launch quantisation from a Live Set (needs a corpus series).
- Recording into slots.
