---
description: How edits reach an engine that is already playing — a new graph built off the audio thread and swapped in at an exact frame
read_when: edit while playing, graph swap, renderer rebuild, Controller::update, live edits, volume change not heard, LOOP-26
type: decision
status: current
last_verified: 2026-10-12
covers:
  - crates/engine/src/lib.rs
  - crates/engine/src/swap.rs
  - crates/engine/src/ahead.rs
  - crates/session/src/lib.rs
---

# 0005: Edits while playing

## Context

A `Renderer` is built from a project once, then moved to the audio thread. Edit commands
(`transport.setTempo`, `track.setVolume`, `track.setPan`, `track.setMute`, `clip.move`, undo and
redo) change the session's project, but a playing renderer did not hear them until the project
was opened again. AGENTS.md rule 3 says how a change has to reach the audio thread: swap whole
graphs atomically, never allocate, lock or free there.

Most of what an edit changes is baked into the renderer's flat lists: a track's volume and pan
are multiplied into every note and audio event of the track, the tempo into every frame
position, and every warped clip placement owns a stretcher. Parameter messages for "cheap"
changes would need a second path for each kind of edit, and would not cover the tempo or a moved
clip; devices (ADR 0004) will add many more parameters.

## Decision

1. **The renderer is split into a graph and its state.** The *graph* is everything derived from
   the project: note and audio events, stretched clip placements and their stretch voices, the
   launcher's prepared clips, lengths and the tempo. The *state* is what playing has produced:
   the playhead, the transport, sounding note voices, playing audio clips, and the launcher's
   runtime (what each column plays, fades and has queued, the beat, the tempo, quantisation).
2. **An edit builds a new graph off the audio thread.** `Controller::update(project)` builds it
   on the calling thread with the audio pool and sample rate the renderer was built with, and
   sends it to the audio thread through a lock-free queue (`rtrb`).
3. **The swap happens at an exact frame.** The controller picks a frame a little ahead of the
   playhead it reads from the monitor (`SWAP_AHEAD`), moves the new graph's stretch voices there
   and fills their rings before sending it. `process` splits its block at that frame, so the
   block before it is the old graph and the frames from it the new one.
4. **State carries over at the swap, without allocation.** The playhead stays in beats: if the
   tempo changed, the transport's origin is moved so the same beat plays on. Cursors into the
   new event lists are found by binary search; playing audio clips are found again by their
   time; an unwarped audio clip plays at its own speed, not the beat's, so it carries on from
   the file position it was at (after a tempo edit its beat position moves, as in other DAWs).
   Sounding note voices ring on, on their track's chain in the new graph. A track's device
   chain keeps its running devices when the new graph has the same devices on the same track,
   with the new parameter values, so instrument notes and effect tails carry on (ADR 0004
   decision 9). The launcher's runtime is copied column by column (a
   column or slot that no longer exists is dropped). A playing launched clip whose prepared
   audio is unchanged keeps its stretch voice (moved from the old graph to the new); otherwise
   the new voice is moved to the clip's position.
5. **Nothing is freed on the audio thread.** The old graph goes back to the controller through
   a second queue and is dropped there, on the next update or when the controller is dropped. If
   that queue is full, the audio thread keeps the old graph and takes no new one until it can.
6. **If the playhead moved unexpectedly** (the transport stopped or started between the build
   and the swap), the swap moves the stretch voices to where the playhead really is
   (`AheadReader::move_to`). Until the worker has done so, stretched clips are silent; they
   never play out of time.
7. **The stretch worker thread serves every graph.** It owns a list of workers; a new graph's
   worker joins it before the graph is sent, and a worker leaves when its graph has been dropped.
8. **The session notifies the player.** After an edit or undo/redo command changes the project,
   `Session::execute` calls `Player::update`, so the CLI, MCP and a future UI all get the same
   behaviour.

## Consequences

- An edit is heard about `SWAP_AHEAD` after the command returns, plus the time to build the
  graph. Building is proportional to the project; stretch voices are the expensive part.
- Every edit rebuilds the whole graph. That is simple and covers every edit and device to come;
  if it becomes too slow for parameter drags in a UI, cheap parameters can get a message path
  later, on top of this one.
- Offline rendering stays the oracle (rule 4): tests swap graphs inside an offline render and
  compare against renders of the edited project.
