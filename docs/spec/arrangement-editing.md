---
description: How arrangement clips behave when moved (along a lane, to another track, over other clips) and how audio clips fade in and out
read_when: moving clips, clip.move, overlapping clips, clip stacking, trimming, splitting, allow sample stacking, allow MIDI stacking, fades, fade-in, fade-out, clip.setFades, fade handles
type: spec
status: draft
last_verified: 2026-10-12
covers:
  - crates/session/src/commands.rs
  - crates/ui/src/timeline.rs
  - crates/ui/src/arrangement_view.rs
  - crates/model/src/lib.rs
  - crates/engine/src/audio.rs
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

`clip.duplicate {track, index}` places a copy right after the clip, replacing what the copy covers
as a moved clip does; `clip.delete {track, index}` removes it. In the Arrangement view a clip's
right-click menu has both, and the keys Ctrl+D and Delete act on the selected clip.

## Copy, cut and paste

`clip.copy {track, index}` keeps a copy of a clip and what it plays; `clip.cut` also deletes it;
`clip.paste {track, start, stack?}` places the copy on a track that takes its kind, replacing what
it covers unless stacked. The pasted clip plays its own copy of the content, so editing one does
not change the other. The window's keys paste after the selected clip, or at the playhead on the
selected track ([shortcuts](../shortcuts.md)).

## Fades

An audio clip on the arrangement has a **fade-in** and a **fade-out**, each a length in beats
(`Clip::fade_in`, `Clip::fade_out` in the model; 0 is no fade). Both are at least 0 and together
at most the clip's length; a project that breaks this does not validate
(`ValidationError::BadFades`), so a renderer refuses it instead of guessing.

`clip.setFades {track, index, in?, out?}` sets one or both, in beats; one undo step. It refuses a
negative length, fades together longer than the clip, a MIDI clip, and a call with neither.
`project.clips` lists each clip's `fade_in` and `fade_out`.

**The curve is linear in amplitude.** `beats` into a clip of `length` beats the clip's gain is

```text
fade_in_gain  = min(1, beats / fade_in)              (1 when fade_in is 0)
fade_out_gain = min(1, (length - beats) / fade_out)  (1 when fade_out is 0)
gain          = fade_in_gain × fade_out_gain × click guard
```

The click guard is the engine's 2 ms linear ramp at both ends of every audio clip; the fades
multiply with it rather than replacing it. The engine works it out per frame from the clip's
start and end in frames (`audio::Fades`), so fades are sample-accurate and the same in offline
renders and real-time playback, unwarped (direct reader) or warped (time-stretched): the envelope
is applied after the stretcher. A fade follows the tempo, since it is in beats. Fades belong to
arrangement clips; a launched clip plays with the click guard only.

When a clip is shortened or split because another clip is placed over it (below), its fades are
fitted to what is left: a split clip's first part keeps the fade-in and the second part the
fade-out, and fades longer together than the part shrink in proportion.

In the Arrangement view an audio clip shows its fades as a line over the waveform, from the
clip's bottom corner to where the fade ends at the top, and the waveform is scaled by the fades.
While the clip is hovered or selected, a small square **handle** sits at its top-left corner
(fade-in) and top-right corner (fade-out); with a fade, the handle sits where the fade ends. A
horizontal drag sets the fade with its end on the grid (Alt held: free), up to the other fade;
the clip shows the fade while it is dragged, and letting go runs `clip.setFades`. A
double-click on a handle removes that fade. Measurements are in
[components](../design/components.md) › Arrangement view.

## Tracks

| Command | Does |
|---|---|
| `track.group {tracks, name?}` | A new group track takes the place of the first track; the tracks (with what is inside them) follow it, play into it, and it plays where they played. They must all be in the same group, or in none. |
| `track.ungroup {track}` | The group goes; its tracks stay where they were and play where the group played. |
| `track.move {track, to}` | Moves a track, a group with everything inside it, to position `to` in the track order. |
| `track.rename`, `track.setColor`, `track.delete` | Name (not empty), colour (`#rrggbb` or `none`), and deleting a track with its clips and slots; deleting a group ungroups it. |

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

- Placing new clips (from the browser, by recording, by duplicating) follows the same rules.
- Moving several selected clips at once.
- Whether a stacked lane should grow taller automatically.
