---
description: How arrangement clips behave when moved: along a lane, to another track, and over other clips (replace or stack)
read_when: moving clips, clip.move, overlapping clips, clip stacking, trimming, splitting, allow sample stacking, allow MIDI stacking
type: spec
status: draft
last_verified: 2026-10-12
covers:
  - crates/session/src/commands.rs
  - crates/ui/src/timeline.rs
  - crates/ui/src/arrangement_view.rs
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
