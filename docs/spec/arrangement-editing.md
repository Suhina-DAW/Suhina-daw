---
description: How arrangement clips behave when moved (along a lane, to another track, over other clips), how audio clips fade in and out, and where playback starts and stops (the play start marker)
read_when: start marker, clip offset, loop start, moving clips, clip.move, overlapping clips, clip stacking, trimming, splitting, clip.split, split at playhead, cut a clip, consolidate, clip.consolidate, join clips, render a range, bounce, allow sample stacking, allow MIDI stacking, fades, fade-in, fade-out, clip.setFades, fade handles, fade shape, fade curve, curve handle, inShape, outShape, inCurve, outCurve, linear, equal_power, fast, slow, s_curve, FadeShape, FadeCurve, play position, play start marker, ruler click, transport.setPosition, stop returns, playhead
type: spec
status: draft
last_verified: 2026-10-15
covers:
  - crates/session/src/commands.rs
  - crates/ui/src/timeline.rs
  - crates/ui/src/arrangement_view.rs
  - crates/model/src/lib.rs
  - crates/engine/src/audio.rs
  - crates/engine/src/launcher.rs
  - apps/suhina-cli/src/play.rs
  - crates/ui/src/menus.rs
  - crates/ui/src/drop.rs
  - crates/session/src/files.rs
  - crates/project-file/src/lib.rs
  - crates/session/src/editing.rs
  - crates/engine/src/track_render.rs
---

# Arrangement editing

How arrangement clips behave when they are moved. The familiar behaviour (observed in Live 12.4,
which this follows) is that a clip placed over others **replaces** what it covers. Some users come
from pattern-based DAWs where clips **stack** and all play; two settings allow that.

## Moving a clip

`clip.move {track, index, start, to_track?, stack?}` moves one arrangement clip to `start` (beats,
at least 0), on its own track or on `to_track`, which must take the clip's kind: a MIDI clip goes
to a track that plays notes, an audio clip to an audio track. A clip that moves to another track
plays through that track's instrument and effects. The move is one undo step.

## Duplicating and deleting

`clip.duplicate {track, index, stack?}` places a copy right after the clip, replacing what the
copy covers unless stacked, as a moved clip does (the window stacks when the setting for the
clip's kind is on); `clip.delete {track, index}` removes it. In the Arrangement view a clip's
right-click menu has both, and the keys Ctrl+D and Delete act on the selected clip.

## Copy, cut and paste

`clip.copy {track, index}` keeps a copy of a clip and what it plays; `clip.cut` also deletes it;
`clip.paste {track, start, stack?}` places the copy on a track that takes its kind, replacing what
it covers unless stacked. The pasted clip plays its own copy of the content, so editing one does
not change the other. The window's keys paste after the selected clip, or at the playhead on the
selected track ([shortcuts](../shortcuts.md)), and stack when the stacking setting for the
clip's kind is on, like a moved clip.

## Splitting

`clip.split {track, index, at}` cuts an arrangement clip in two at song position `at` (beats from
the beginning), which must be inside the clip (not on its edges). It is one undo step and returns
both parts' indices (`first`, `second`).

- **Both parts play the same content** (one content in the pool, as before the cut), so editing
  the notes or the audio of one changes the other, as for a duplicated clip.
- **The second part carries on from where the clip was at the cut:** its start marker
  (`offset`) is the content position the clip had reached there, following the loop (a clip
  whose start marker is at or past the loop end starts at the loop start) and, for a transposed
  audio clip that is not warped, the faster or slower speed it moves through its file at
  (`2^(transpose / 12)` content beats per beat). Loop, warp markers, warp mode, gain and
  transpose are the content's or the clip's, and stay as they are.
- **Fades:** the first part keeps the fade-in and the second the fade-out, each with its shape
  and curve; there is no fade at the cut (a zero-length fade, so only the engine's 2 ms click
  guard is there). A fade longer than its part shrinks to fit it, as when a clip is shortened.
- **Notes across the cut end there.** A note that starts before the cut belongs to the first
  part and is cut at its end; the second part does not start it again (notes are not picked up
  mid-way, as when playback starts inside a clip). Notes that start at or after the cut play in
  the second part.

**The renders before and after a split are the same,** sample for sample, for a MIDI clip with
no note across the cut. An audio clip differs only around the cut: within the click guard on
both sides (2 ms), and for a warped clip, whose second part starts its time-stretcher at the cut
without the audio before it, very slightly for its first moments, as when playback starts there
(Play position).

In the Arrangement view **Ctrl+E** splits the selected clip at the playhead (nothing happens,
with a hint in the status bar, when the playhead is not inside it), and a clip's right-click
menu has **Split here**, which cuts where the clip was right-clicked, on the grid clips snap to
at that zoom (Alt held while right-clicking: exactly there).

## Consolidating

`clip.consolidate {track, start, end}` turns the range `start … end` (beats) on one track into
one new clip there. It is one undo step and returns the clip's `index`, and for audio the `file`
written.

- **What is in the range:** the enabled clips on the track's lane that overlap it. A clip
  reaching in from before or past the range counts with the part inside it; clips outside are
  unchanged. As with a split, a note that starts before the range is left out.
- **MIDI clips** are joined: the new clip's content holds every note the clips play inside the
  range, where it sounds (after start markers, loops and clip ends are applied), each cut where
  its clip ends or the range ends. Gaps between clips are silent.
- **Audio clips** are rendered: the track's clips play through the engine's renderer
  (`suhina_engine::render_track`, the same `process` as playback) with their own gain, fades,
  warping and transpose, but no devices, no track volume, pan or mute and no other track, at
  unity gain. Gaps are silent. The audio is written as a 32-bit float stereo WAV file to a
  folder **`Consolidated`** next to the project file, named after the track and the bar the range
  starts on (`3 Audio bar 5.wav`; ` 2`, ` 3` … are added when that file exists), at the highest
  sample rate of the files rendered. The new clip plays that file unwarped, at gain 0 dB, without
  fades. A project that has not been saved has no folder for it, so the command asks to save it
  first. Undo removes the clip but leaves the file on disk.
- **The new clip** starts at `start`, is `end − start` long, has its start marker at 0, no loop
  and no fades, and replaces what the range held (the clips under it are shortened, split or
  removed as for a clip placed there, whatever the stacking settings). Its content is named
  after the first clip's.
- **Refused:** a range that is empty, negative, past 120 000 beats or longer than an hour of
  audio from the earliest clip in it; a range with no clips; MIDI and audio clips together; a
  clip that plays on other tracks (a shared pattern).

The render of the project before and after a consolidation is the same, apart from the click
guard at the range's edges when audio is playing there, since the new clip starts and ends
there. An audio clip consolidated plays at the tempo it was rendered at.

In the Arrangement view **Ctrl+J** consolidates the selected clip (its own start to its end,
with any clips stacked over it), and a clip's right-click menu has **Consolidate**. Consolidating
a single clip makes its loop, start marker, fades and warping one plain clip.

## Fades

An audio clip on the arrangement has a **fade-in** and a **fade-out**, each a length in beats
(`Clip::fade_in`, `Clip::fade_out` in the model; 0 is no fade). Both are at least 0 and together
at most the clip's length; a project that breaks this does not validate
(`ValidationError::BadFades`), so a renderer refuses it instead of guessing.

Each fade also has a **shape** and a **curve** (`Clip::fade_in_curve`, `Clip::fade_out_curve`:
a `FadeCurve` of a `FadeShape` and a number). The default is linear with curve 0 at both ends,
so a project without them sounds as it did before shapes existed.

`clip.setFades {track, index, in?, out?, inShape?, outShape?, inCurve?, outCurve?}` sets any of
them, lengths in beats; one undo step. It refuses a negative length, fades together longer than
the clip, an unknown shape, a curve outside −1 … +1, a MIDI clip, and a call with none of them.
`project.clips` lists each clip's `fade_in` and `fade_out` and their shapes and curves
(`fade_in_shape`, `fade_in_curve`, `fade_out_shape`, `fade_out_curve`).

**Shapes.** Over a fade, `t` runs from 0 at its silent end to 1 at full level: `t = beats /
fade_in` for the fade-in, and `t = (length − frame − beats) / fade_out` for the fade-out, where
`frame` is the length of one frame in beats. A fade-out is therefore the mirror image of a
fade-in with the same shape. The shape gives the gain `s(t)`:

| Shape | `s(t)` | At ½ | Character |
|---|---|---|---|
| `linear` | `t` | 0.5 (−6 dB) | Straight line in amplitude |
| `equal_power` | `sin(π t / 2)` | 0.707 (−3 dB) | A fade-out and a fade-in of the same length over each other keep the power constant |
| `fast` | `1 − (1 − t)²` | 0.75 | Rises quickly, then levels off |
| `slow` | `t²` | 0.25 (−12 dB) | Stays low, then rises |
| `s_curve` | `t² (3 − 2t)` | 0.5 | Flat at both ends, steepest in the middle |

**Curve.** The curve `c`, −1 … +1, bends the shape: `gain = s(t)^(4^(−c))`. At 0 the gain is the
shape exactly; +1 raises it to the power ¼ (a faster start, a logarithmic feel), −1 to the power 4
(a slower start, an exponential feel). Every shape and curve gives 0 at `t = 0` and 1 at `t = 1`,
and rises in between.

On the frame `beats` into a clip of `length` beats the clip's gain is

```text
fade_in_gain  = gain(fade_in_curve, min(1, beats / fade_in))                      (1 when fade_in is 0)
fade_out_gain = gain(fade_out_curve, min(1, (length - frame - beats) / fade_out)) (1 when fade_out is 0)
gain          = fade_in_gain × fade_out_gain × click guard
```

The fade-in counts the time before the frame and the fade-out the time after it, so the first and
the last frame are both silent. The click guard is the engine's 2 ms linear ramp at both ends of
every audio clip, counted the same way, and the fades multiply with it rather than replacing it. The engine works it out per frame from the clip's
start and end in frames (`audio::Fades`), so fades are sample-accurate and the same in offline
renders and real-time playback, unwarped (direct reader) or warped (time-stretched): the envelope
is applied after the stretcher. A fade follows the tempo, since it is in beats. Fades belong to
arrangement clips; a launched clip plays with the click guard only.

When a clip is shortened or split because another clip is placed over it (below), its fades are
fitted to what is left: a split clip's first part keeps the fade-in and the second part the
fade-out, and fades longer together than the part shrink in proportion.

In the Arrangement view an audio clip shows its fades as a line over the waveform, from the
clip's bottom corner to where the fade ends at the top, following the fade's shape and curve,
and the waveform is scaled by the fades.
While the clip is hovered or selected, a small square **handle** sits at its top-left corner
(fade-in) and top-right corner (fade-out); with a fade, the handle sits where the fade ends. A
horizontal drag sets the fade with its end on the grid (Alt held: free), up to the other fade;
the clip shows the fade while it is dragged, and letting go runs `clip.setFades`. A
double-click on a handle removes that fade. A right-click on a handle opens a menu of the shapes,
with the fade's shape checked; choosing one runs `clip.setFades`. A fade at least 21 px wide has a
second, round **curve handle** on its line half-way through the fade. Dragging it up or down
moves it with the pointer: the curve becomes the one whose line passes through the handle (up
for +, down for −), until it reaches −1 or +1, the line following live, and letting go runs
`clip.setFades`; a double-click sets the curve back to 0. The view draws the fades per pixel
with `t` in beats (`length − beats` for the fade-out), without the one-frame term above, which
is less than a pixel at any zoom. Measurements are in
[components](../design/components.md) › Arrangement view.

## Start marker and loop

A clip plays from its start marker (`Clip::offset`, in content beats) and, with a loop, around
the loop from then on. Live keeps the start marker inside the loop: moving the loop moves the
marker with it ([als-format](als-format.md) › Markers). **Suhina:** a project whose start marker
is at or past the loop end starts at the loop start, in every reader alike (notes, audio read
directly or time-stretched, and launched clips).

## Play position

The transport starts from the **play start marker**, a position in beats (0 when a project
opens). While stopped, the playhead sits on the marker.

**Where other DAWs stand** (their public manuals, October 2026): every one starts playback from a
start position the user sets; they differ on the first Stop.

| | First Stop | Stop again, while stopped | Setting |
|---|---|---|---|
| Live | back to where playback started (the insert marker) | the marker to the start | none |
| Bitwig Studio | stays: the start position moves to the playhead | playhead and start position to the start | none |
| Logic Pro | as the Stop button's menu says: stay, back to the left locator, or back to the last start | to the project start | the Stop button's menu |
| Cubase | back to the start position, or stays ("Return to Start Position on Stop") | back to the start position | Preferences › Transport |
| REAPER | Space: back to the start; Ctrl+Space: stays (pause) | | two keys |

Suhina takes the common ground: Stop goes back to the marker by default, staying put is a
setting, and a second Stop goes to the beginning.

**Commands.**

| Command | Does |
|---|---|
| `transport.setPosition {beats}` | Moves the marker to `beats` (0 … 120 000; a negative, too large or non-numeric value is an error). Stopped, the next `transport.play` starts there. Playing, playback jumps there at once. |
| `transport.play` | Starts the transport from the marker; nothing happens while it runs. Launching a clip or scene while stopped also starts the arrangement from the marker. |
| `transport.stop {return?}` | Stops everything at once. `return: "marker"` (the default): the playhead goes back to the marker. `return: "here"`: the marker moves to where the playhead stopped, so the next start carries on from there. Either way, stopping while already stopped moves the marker to beat 0. |

`launcher.state` reports `start_beat` (the marker) next to `beat` and `position` (the playhead).

**Starting inside the arrangement.** From the marker's frame on, the arrangement plays exactly as
it would at that frame of a render from the beginning, with two exceptions: a note that started
before the marker is not played (notes are not picked up mid-way), and a time-stretched audio clip
starts its stretcher at that point, without the audio before it, so it can differ very slightly
for its first moments. Unwarped and re-pitched audio clips that span the marker play from where
they are, sample for sample. The metronome counts from the marker: on a beat it clicks at once,
between beats it waits for the next one. The launcher's beat is the playhead's, so quantised
launches land on the arrangement's bars.

**A jump while playing** ends the notes that sound (with their release) and the audio clips that
play, and plays on from the new beat as a start would. Launches and a scene tempo that were
waiting are queued again for the next boundary from the new beat. Time-stretched clips are
silent until the stretch worker has caught up with the new place (about one block in practice).

**In the Arrangement view** a click on the ruler runs `transport.setPosition`: the beat under the
pointer, on the same grid clips snap to at that zoom (Alt held: free, to a thousandth of a beat).
The marker is a small triangle hanging from the ruler's lower edge; the playhead is a line through
the ruler and the lanes, at the marker while stopped (measurements in
[components](../design/components.md) › Arrangement view). The Stop button and the play/stop key
pass the setting **Stop** (Settings › General): **Marker** (default) or **Stay**, saved as
`stop = marker | here`.

**From the command line** (`suhina-cli play --launcher`): `stop` returns to the marker,
`stop here` stays, `pos <beats>` moves the marker. Agents pass `return` themselves; without it,
Stop returns to the marker.

## Tracks

| Command | Does |
|---|---|
| `track.group {tracks, name?}` | A new group track takes the place of the first track; the tracks (with what is inside them) follow it, play into it, and it plays where they played. They must all be in the same group, or in none. |
| `track.ungroup {track}` | The group goes; its tracks stay where they were and play where the group played. |
| `track.move {track, to}` | Moves a track, a group with everything inside it, to position `to` in the track order. |
| `track.rename`, `track.setColor`, `track.delete` | Name (not empty), colour (`#rrggbb` or `none`), and deleting a track with its clips and slots; deleting a group ungroups it. |
| `track.add {kind?, name?, after?, device?, preset?, voice?}` | A new audio or MIDI track with its channel, slots and lane, after `after` or after the last regular track ([browser](browser.md) › Commands). |

Clips placed from the browser (`clip.addFiles`) replace what they cover in the Arrangement like a
moved clip; they stack when either stacking setting is on (one `stack` flag for the whole drop,
not per kind) ([browser](browser.md) › Files).

In the Arrangement view a header click selects a track (Ctrl adds), Ctrl+G groups the selection,
dragging a header moves the track above the lane under the pointer (below it from its lower half),
and a right-click opens the track's menu: name, colour, group, ungroup, delete. The Session view's
column headers have the same menu.

## Over other clips

The moved clip covers `[start, start + length)` on its lane. Every other clip on that lane:

| Where it is | Replace (default) | Stack |
|---|---|---|
| Outside the moved clip | unchanged | unchanged |
| Entirely under it | removed | unchanged |
| Starting before it, ending inside | shortened to end where the moved clip starts | unchanged |
| Starting inside, ending after | starts where the moved clip ends; its content keeps playing from where it was at that point | unchanged |
| Starting before and ending after | split in two around the moved clip, the second part continuing the content from where it was | unchanged |

A shortened or split clip keeps fades that fit it (see Fades).

"Where the content was" follows the clip's loop: a looping clip continues inside its loop, so the
second part sounds as the original did at that moment.

Replace and stack are chosen per move (`stack: true` stacks). The app passes the user's settings:
**Allow sample stacking** for audio clips and **Allow MIDI stacking** for MIDI clips, both off by
default.

## Showing stacked clips

Where clips on a lane overlap, the lane shows them side by side in sub-rows (each clip in the
first sub-row free at its start), so every clip stays visible and can be grabbed. A lane without
overlaps shows one row.

## Open

- Placing clips by recording follows the same rules (clips from the browser, duplicating and
  pasting already do).
- Moving several selected clips at once.
- Whether a stacked lane should grow taller automatically.
- Dragging along the ruler to scrub, and a loop brace.
- Consolidating more than one selected clip, or a time selection; the window consolidates the
  selected clip's range until there is a selection of several clips.
- A consolidated audio clip warped to the project, so it follows a later tempo change.
