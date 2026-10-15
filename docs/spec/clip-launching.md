---
description: How launching clips and scenes behaves: quantisation, stop buttons, scene tempo, launched clip fades, follow actions, launcher over arrangement
read_when: clip launching, scenes, launch quantisation, stop, session view, launcher commands, fades on launched clips, follow actions
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/engine/src/launcher.rs
  - crates/engine/src/follow.rs
  - crates/engine/src/follow_tests.rs
  - crates/session/src/follow.rs
  - crates/session/tests/launcher.rs
  - crates/engine/src/fade_tests.rs
  - crates/session/tests/scenes.rs
  - crates/ui/src/menus.rs
  - crates/session/tests/follow.rs
  - crates/session/tests/files.rs
---

# Clip launching

- **Sources:** the behaviour of the Session View as the public Live 12 manual describes it
  (launching clips and scenes, launch quantisation, stop buttons, scene tempo), and the Live Set
  fields in [`als-format.md`](als-format.md) (scenes, clip slots, `HasStop`, scene tempo, clip
  markers; corpus series 1 and 4).
- **Observed with:** nothing yet. The launch behaviour has not been checked against Live 12.4.6
  by listening or watching; every statement marked **(hypothesis)** needs that check (see
  *To observe* below). The Live Set fields come from the corpus and are observed.
- **Status:** first version, implemented in `crates/engine/src/launcher.rs` (ROADMAP M4);
  follow actions in `crates/engine/src/follow.rs`. Per-clip launch quantisation and launch modes
  (trigger, gate, toggle, repeat) are not specified yet; they need a corpus series of their own.

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
   triggered.**)** A scene's time signature changes the time signature at the same moment.
   **Suhina:** the new time signature's first bar starts at the launch moment, so bar
   quantisation and the metronome's bar click count from there; note-value quantisation stays on
   the beat grid from beat 0. The bar number keeps rising across the change (a bar cut short by
   it counts as one). Stopping the transport keeps the tempo and time signature a scene set and
   counts bars from the start again; editing the project's own tempo or time signature replaces
   them.
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

## Editing scenes

- **Insert** (`scene.insert`) adds an empty scene at a position (after the last one if none is
  given): a row of empty slots, each with a stop button, in every track.
- **Duplicate** (`scene.duplicate`) copies a scene to a new one right below it: name, tempo, time
  signature and every slot. **Suhina:** each copied clip plays a copy of its content, so editing
  one clip does not change the other (as `clip.paste` does).
- **Delete** (`scene.delete`) removes a scene and the clips in its row. **Suhina:** the last
  scene left cannot be deleted, so the launcher always has a row to put clips in.
- **Rename** (`scene.rename`); an empty name clears it, and the button shows the number only.
- **Tempo** (`scene.setTempo`, 20 to 999 bpm) and **time signature** (`scene.setTimeSignature`,
  numerator 1 to 99, denominator 1, 2, 4, 8 or 16); leaving the value out clears it, and the
  scene then keeps what is playing.
- Every one of these is an edit with one undo step. **While playing**, a clip that plays or is
  queued stays with its row when a scene is inserted or deleted above it (and when that is
  undone), so the same clip keeps playing under its new scene number; deleting the playing
  scene stops its clips (with the stop fade). Edits to a scene's tempo or time signature apply
  at its next launch.

## Follow actions

A slot clip can say what happens after it has played for a while, so clips play one after
another without being launched by hand. The general idea (an action A and an action B, picked by
chance after a time) comes from the public manual's description; every detail below is Suhina's
own decision (**Suhina:**), not yet compared with Live 12 (see *To observe*).

1. **Settings.** Each clip has a follow action, off by default: a **follow time** in beats, or
   **linked** to the clip's length (the `length` that `project.launcher` lists); **action A** and
   **action B**; and a **chance** for each, a weight from 0 to 100. Arrangement clips keep the
   settings but never run them.
2. **When.** The follow time counts from the moment the clip was launched, which is already on
   the quantisation grid; the action itself is **not** quantised again and happens on the exact
   frame the time is reached. **Suhina:** a clip without a loop that ends before its follow time
   runs its action at its end, so a chain of one-shots does not stall in silence; a follow time
   shorter than 1/64 beat counts as 1/64 beat.
3. **A or B.** At the follow time the launcher picks A with the chance
   `chance A / (chance A + chance B)`, else B. Both chances 0 means no action. The action runs
   once per launch: a clip that keeps playing ("nothing") does not try again a follow time later.
4. **Actions.** In the clip's own column:
   - **nothing:** the clip keeps playing;
   - **stop:** the column stops (with the stop fade);
   - **again:** the clip is launched again from its start;
   - **previous** / **next:** the nearest clip above / below that can play, wrapping around the
     column; when the clip is alone in its column it is launched again;
   - **first** / **last:** the top-most / bottom-most clip that can play;
   - **any:** a clip that can play, picked at random, this one included;
   - **other:** another clip that can play, picked at random; this one again when it is alone;
   - **jump N:** the clip in scene N's slot; when that slot has no clip that can play, the clip
     keeps playing.

   **Suhina:** empty slots (with or without a stop button) and deactivated clips are skipped;
   they are never a follow action's target.
5. **Launched by hand wins.** When a launch or stop is already queued in the column at the follow
   time, the follow action is skipped and the queued one happens on its boundary.
6. **Random numbers.** Offline renders start from a fixed seed, so the same project and the same
   launches always render the same picks (the engine's tests rely on it). Playback through the
   audio device seeds from the clock when the player is attached. **Suhina:** the
   `SeedRandom` engine command sets the seed.
7. **Commands.** `clip.setFollowAction {track, scene, enabled?, time?, linked?, actionA?,
   actionB?, chanceA?, chanceB?}` changes what it is given, in one undo step; actions are named
   `nothing`, `stop`, `again`, `previous`, `next`, `first`, `last`, `any`, `other` or `jump N`
   (N a scene number, from 1). `project.launcher` lists each slot clip's `follow_action`. The
   project file stores it as the clip's `follow` ([suhina-format](suhina-format.md)). The
   detail area shows a selected slot clip's follow action and changes it through the same
   command ([Session view](ui-session-view.md) › Detail area: follow action).
8. **While playing.** Editing a follow action swaps in a new graph (ADR 0005); a clip that is
   playing keeps its launch time, so a follow time now shorter than what it has played runs at
   once.

## Suhina decisions

- **Files dropped into a slot loop.** A sample, a MIDI file's track or a Stems file's part placed
  in a slot (`clip.addFiles` with `scene`) loops over its whole length, a sample with no known
  tempo too: unwarped, it plays the whole file at its own speed and starts it again
  ([browser](browser.md) › Warping says why the rule is "always").
- **Loop switch.** `clip.setLoop {track, scene, loop}` switches the loop of the clip in a slot
  (one undo step). On, it repeats its whole length (or the loop it already has); off, it plays
  once from its start to its end marker and the column stops (behaviour 4). A clip that is
  playing follows the switch at once, like any edit while playing
  ([ADR 0005](../decisions/0005-edits-while-playing.md)).
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
  of the next audio block. `launcher.state` reports the time signature playing and the position
  counted in it. **Suhina:**
  `launcher.state` also reports the browser's preview (`preview`: playing, seconds), which plays
  beside the launcher and never changes it ([ADR 0011](../decisions/0011-preview-and-added-audio.md)),
  and the engine's CPU load (`cpu`: percent), which the control bar shows. `transport.stop`
  with `return: "here"` stops and keeps the position where the transport stopped
  ([arrangement editing](arrangement-editing.md) › Play position).
- **A new audio output.** When the output is opened again while playing (another device or
  buffer size chosen, a device that reported an error, the chosen device back), the launcher
  goes on as it was: each column's launched clip plays on from as far into it as it had played
  (`LaunchCommand::ResumeColumn`), a column stopped over its arrangement stays silent, launches
  and stops that were waiting wait for the next boundary again, and the launch quantisation and
  a scene's tempo carry over (`LaunchCommand::SetLaunchTempo`). Only the moment the device needs
  to open is lost.

## Clips between the arrangement and the slots

**Suhina**, our own design: a clip gets into a slot from the arrangement, and from a slot onto
the arrangement, by copying ([Session view](ui-session-view.md) › Between the views).

- `clip.copy {track, scene}` copies the clip in a track's slot (scene from 1); `clip.copy
  {track, index}` an arrangement clip. Both fill the same clipboard.
- `clip.paste {track, scene}` puts the copied clip in that track's slot: the track must have clip
  slots (a group or return track has none), the scene must exist, and the track must take the
  clip's kind (notes or audio). A clip already in the slot is replaced; the slot keeps its stop
  button. One undo step; an error changes nothing and records nothing.
- The slot's clip plays its own copy of the content (gain, warping and transpose come with it) and
  keeps the clip's length, offset, loop, enabled state and fades; its start is 0. Fades that
  do not fit the clip are fitted as when a clip is shortened.
- The other way, `clip.paste {track, start, stack?}` places a slot's clip on the arrangement, as
  an arrangement clip is pasted ([arrangement editing](arrangement-editing.md) › Copy, cut and
  paste).
- In the window: Copy and Paste in a slot's or a clip's right-click menu, Ctrl+C and Ctrl+V on the
  selected slot or clip, and dragging a clip over the other view's name in the control bar.
- A slot clip's fades are kept, but launched clips do not play them yet (Suhina decisions ›
  Fades above).

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
- Follow actions: is the follow time counted from the quantised launch, is the action itself
  quantised, what "next" does at the bottom of a column and on empty slots, and what a one-shot
  shorter than its follow time does. Where a Live Set stores a clip's follow action, so the
  importer can read it.

## Not yet

- Launch modes, per-clip launch quantisation, legato.
- Follow actions: reading them from a Live Set, a follow-action multiplier or count, scene
  follow actions, and controls for them in the detail area (they are set by command for now).
- Reading the global launch quantisation from a Live Set (needs a corpus series).
- Reading a scene's time signature from a Live Set: [als-format](als-format.md#scenes) notes it
  is there, but not its elements yet.
- Capturing the playing clips into a new scene.
- Recording into slots.
- Moving a clip between a slot and the arrangement (a cut and paste works; a drag copies).
