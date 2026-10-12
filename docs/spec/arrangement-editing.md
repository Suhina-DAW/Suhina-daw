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
