---
description: Musical time (beats) is the model's only time base, and tempo and time signature over the song are an append-only list of segments, so a tempo change never moves a note and two tempo maps merge as a union
read_when: tempo map, tempo change, time signature change, musical time, beats, tick, frames_per_beat, transport, scene tempo, warp markers, tempo segment, append-only, merge, issue 402
type: decision
status: draft
last_verified: 2026-10-15
covers:
  - crates/model/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/engine/src/lib.rs
  - crates/session/src/commands.rs
---

# 0024: Musical time and tempo over the song

- **Status:** proposed
- **Date:** 2026-10-15
- **Task:** tempo and time-signature changes over the song
  ([#402](https://github.com/suhina-daw/suhina-daw/issues/402), label `engine`) needs a
  representation before it can be implemented. The shape decided here is also what keeps a tempo map
  mergeable, so it is decided once rather than twice; the sibling decisions are
  [0023](0023-collaboration-and-project-versions.md) (the shape of sharing and versions),
  [0025](0025-object-identity.md) (object identity) and
  [0026](0026-project-folder-and-package.md) (the project folder and the format version).

## Context

A project has exactly one tempo and one time signature. `model::Transport` holds `tempo_bpm: f64`
and a `TimeSignature`, and `Project` holds one `Transport` (`crates/model/src/lib.rs`: `Project` at
line 24, `Transport` at line 36, `TimeSignature` at line 48). That is enough for a loop and a clip
grid. It is not enough for a song: music changes tempo and metre, and the launcher already carries a
per-scene tempo override (`Scene { name, tempo_bpm: Option<f64> }`,
`crates/model/src/lib.rs:467`), so the model already admits that the tempo is not one number.

Three things force the question now rather than later.

**A tempo change already has to survive the engine, and only the model has not said so.**
[ADR 0005](0005-edits-while-playing.md) builds a new graph after an edit and swaps it in at a block
boundary, carrying the playhead over "on the same beat when the tempo changed". Musical time is
therefore already how the engine survives a change of tempo. What is missing is the model saying
that this is the invariant rather than an implementation detail.

**Being wrong here is a model-wide rewrite.** Every position in the model is a position in time:
clip starts, lengths and offsets, loop ranges, fades, note times, warp markers, the playhead.
[ADR 0001](0001-daw-neutral-model.md) fixed the unit as `f64` beats and left the door open — "if
rounding errors appear against tick-based formats (FL uses PPQ ticks), we revisit this with a
rational or tick-based time type". This ADR closes that door deliberately and says why now, while
there is no installed base of project files to migrate (see [ADR 0026](0026-project-folder-and-package.md)).

**A tempo map is the hardest thing in the model to merge, unless it is built so that merging is a
union.** If Suhina ever shares a live session or merges two versions of a project
([0023](0023-collaboration-and-project-versions.md)), the tempo map is the part with the least
tolerance for a heuristic: two machines disagreeing about where the beat is cannot be resolved by
picking one side. The shape that merges without conflict costs nothing extra today and is a rewrite
later, so it is decided here rather than at the point collaboration lands.

## Decision

1. **Every position in the model is musical time, in beats. Never wall-clock time, never seconds,
   never audio samples.** This is an invariant of `suhina-model`, not a convention.

   It is enforced in three ways that can be checked rather than trusted:

   - The `model` crate has no notion of a sample rate and no frame, sample or second field anywhere,
     so a position *cannot* be expressed in any other unit without adding one.
   - Conversions to and from frames happen only in the engine, only while a graph is built, and only
     through the tempo in force (decision 8). The audio thread never converts.
   - The same project rendered at two sample rates has the same musical timing, tested as a golden
     comparison in beats rather than in frames.

   The reason is not tidiness. A tempo change must not move a note: with positions in beats, a note
   at beat 12 stays at beat 12 when the tempo changes, and only *when* beat 12 is heard changes.
   With positions in samples or seconds, a tempo change moves every note that follows it, and no
   amount of care in the engine repairs a model that has already stored the wrong thing. The second
   reason is that only musical time survives being replicated to a second machine: two machines'
   audio clocks differ by tens of ppm, so a position in seconds means something different on each,
   while a position in beats means the same thing on both.

2. **Tempo and time signature over the song are an append-only list of segments.** A segment names a
   point and carries the tempo and the time signature in force from that point on. Changing the
   tempo **appends a new segment**; nothing already written is edited or removed.

   The reason, stated prominently because it is the whole point: **append-only is what makes the
   list mergeable.** Two machines that each append a segment have a union, not a conflict, and
   neither has rewritten a segment the other has already seen. A list that is edited in place has to
   be compared and reconciled; a list that only grows can be merged by set union, which needs no
   clock, no ordering and no agreement about who is right. This is the same property the sharing and
   versions work depends on ([0023](0023-collaboration-and-project-versions.md)), which is why the
   representation is decided here rather than there.

   A segment is a **state at a point, not a delta**: it repeats the time signature even when only the
   tempo changed. The consequence is that a reader can interpret any segment without reading the ones
   before it, so a partial list is still readable and two segments can be compared directly. The cost
   is a few repeated bytes per segment.

   This is the model Ableton Live's Link publishes for tempo changes: a tempo change is not an edit to
   a number but a new timeline segment, a linear function between beats and time that crosses the
   previous one at the point where the change happens (Ableton, *Link features and functions FAQ*,
   <https://help.ableton.com/hc/en-us/articles/209776125-Link-features-and-functions-FAQ>; taken from
   that published description, not from code, per AGENTS.md rule 1). We adopt the idea, not the
   implementation: Link needs it so that independent applications agree on where the beat is, and
   Suhina needs it for the same reason plus the merge property above.

3. **A segment's point is a beat.** The alternative — a point in seconds or in host time — was
   rejected (see *Alternatives considered*). A point in beats is what a musician names ("the tempo
   change is at bar 32"), it is the same unit as every other position in the model, and it is what
   keeps the list independent of any one machine's clock. The tradeoff is real and is accepted: with
   points in beats, inserting a segment *earlier* in the song changes when the later segments are
   heard, because their beat positions are unchanged while the beats before them now take a different
   amount of time. That is the correct behaviour for a project whose notes are also in beats — the
   music stays on the same beats and everything moves together — but it means a tempo map is not a
   set of independent wall-clock appointments, and the UI must not present it as one.

4. **When two segments name the same point, the later one is the one in force.** This is what lets a
   tempo be *revised* without editing what is already written: appending a segment at a point that
   already has one supersedes it in effect while leaving the list append-only. A reader takes, for
   each point, the last segment at that point. The list is kept ordered by point, and ordering is
   part of what the format writes so that two files that mean the same thing are equal.

5. **A project always has at least one segment**, conventionally one at beat 0 carrying the project's
   tempo and time signature. A project whose list is empty is invalid (`Project::validate`), so the
   tempo in force is defined at every point of the song and no reader has to invent a default.

6. **The scene tempo stays an override and never writes a segment.** `Scene::tempo_bpm` is unchanged:
   launching a scene with a tempo puts that tempo in force for the engine, and the song's map is not
   touched. The reason is [ADR 0002](0002-commands.md): a transport command "never change[s] the
   project and record[s] no undo step". If launching a scene appended a segment, then a performance
   action would be an edit, it would need an undo step, and pressing Undo after a scene launch would
   undo the launch rather than the user's last edit. The map is changed only by edit commands. How
   long an override stays in force (until the transport stops, or until the next launch that names a
   tempo) is the launcher's behaviour and belongs in
   [`docs/spec/clip-launching.md`](../spec/clip-launching.md), not here; what is decided here is only
   that it does not mutate the map.

7. **`frames_per_beat` is derived per graph and is never stored.** It is computed from the tempo in
   force when a graph is built, and lives only in the engine and its monitor. It must not appear in
   `model`, in the project file, or in any command's parameters: it is a function of the tempo, the
   sample rate and the frame budget, all of which already exist. Storing it would create a second
   source of truth that a sample-rate change or a tempo edit would have to remember to update.

8. **Migration from today's single tempo.** A project with one tempo becomes a list with one segment
   at beat 0. This changes the project file layout, so it is a **format change**: `project-file` gains
   its lines for the new representation and the round-trip test gains its case (ADR 0010: "Every new
   field in the model needs its line in `suhina-project-file` and in the round-trip test"), and a file
   written under the old layout must still open and become one segment. **The format version bump
   itself is decided in [ADR 0026](0026-project-folder-and-package.md)**, not here; this ADR only
   records that the model change cannot ship without it.

9. **Reading a position is never an equality test.** Positions are `f64` beats, so comparisons use a
   tolerance rather than `==`, as the model already does for fades (`FADE_EPSILON`,
   `crates/model/src/lib.rs:337`, "for rounding in beat arithmetic"). Decision 1's test — a tempo
   change does not move a note — is written with that tolerance, and a segment's point is compared
   the same way when the list is kept ordered.

## Consequences

- **`model`.** `Transport` gains the segment list; `TimeSignature` gains its place inside a segment
  and is no longer a member of `Transport` on its own; `Project::validate` gains rules for the map
  (at least one segment, a finite and positive tempo, an ordered list) and a `ValidationError`
  variant per rule, so a malformed map is an error rather than a crash (AGENTS.md rule 2).
  `Scene::tempo_bpm` is unchanged.
- **`project-file`.** The list is written and read field by field as the crate already does for every
  other model value, and a project written before this change reads as a single segment at beat 0.
  The round-trip test must show that a project with a map is equal to itself after a save and open,
  and that an old file becomes the expected one-segment map.
- **`engine`.** A graph already reads one tempo and derives `frames_per_beat` from it; it now reads
  the tempo in force at the block it is rendering, which means a graph that spans a segment boundary
  must be able to advance the tempo in force without being rebuilt. Whether that is done by rebuilding
  the graph at the boundary or by carrying the segments into the graph is an engine implementation
  question, not a model one; what the model guarantees is that a segment boundary is a position in
  beats and therefore exact.
- **`session`'s commands.** Reporting commands expose the map; changing it is one or more new edit
  commands, each with exactly one undo step (ADR 0002). No transport command touches it (decision 6).
- **`ui`.** The control bar's tempo field and the arrangement ruler both have to answer "the tempo at
  this position, and can I change it here" — that is behaviour, and it is written down in a spec before
  it is drawn (`docs/spec/`, per AGENTS.md rule 1 and the documentation table).
- **This is independent of collaboration and lands whether or not collaboration ever ships.** The
  mergeability argument in decision 2 is a reason to choose this shape now; it is not a reason to wait
  for the feature that needs it. A project gains a tempo map the moment #402 is implemented, and a
  solo user gets the whole benefit.
- **It is decided now because the cost of being wrong grows.** ADR 0001's escape hatch to a tick-based
  time type stays open in principle but is closed in practice by this ADR: changing the time base later
  would touch every position in the model, every reader and writer, every command that takes a
  position, and every golden render. Doing it while no `.suhina` file exists in the wild is nearly
  free; doing it afterwards is the largest single change the model can undergo.
- **Tests it needs** (AGENTS.md rule 7):
  - a tempo change does not move a note: the same project rendered before and after a tempo change
    has its note onsets at the same beats, within the tolerance of decision 9;
  - a note *after* a tempo change is heard at the right wall-clock time, and a note before it is not
    moved;
  - the launcher follows the map, and a scene tempo overrides it without changing the project — the
    project before and after a scene launch is equal;
  - the playhead carries over on the same beat when the tempo changes (the ADR 0005 behaviour,
    now asserted against a map rather than a single tempo);
  - the map round-trips through the project file, and a file written before this change becomes one
    segment at beat 0;
  - a hostile map — empty, unsorted, a non-finite or non-positive tempo, a point before beat 0 —
    validates as an error and never panics;
  - the same project rendered at two sample rates has the same timing in beats.
- **Documents to update in the same change** (`python scripts/docs.py impact`): a new
  `docs/spec/tempo-map.md` for the behaviour (the UI, the commands, what a segment shows), `ROADMAP.md`,
  [`docs/spec/als-format.md`](../spec/als-format.md) where it describes what a Live Set's tempo
  automation means, and the reference to #402. `docs/commands.md` is regenerated on `main`.

## Alternatives considered

**(a) Keep a single tempo, and have no map.** The status quo, and the cheapest by far: no model
change, no format change, nothing to merge. It is rejected because it cannot express the music the
project is for. Concretely, three things break: a song with a ritardando, a half-time section or a
metre change cannot be written at all; a clip or scene that wants a different tempo has nowhere to
put it, so the launcher's `Scene::tempo_bpm` stays a special case that does not generalise; and every
future feature that needs a tempo at a position — a metronome, a grid, warp markers, exported tempo
automation — has to carry its own private notion of a tempo change, which is how a model acquires
five incompatible answers to one question. The map is the cheaper option as soon as one of those
features lands, and #402 is that feature.

**(b) Write the changes as absolute seconds, or as host time.** This is what a scheduling system
would do: each change names a wall-clock instant, and the tempo changes there. It is rejected because
it makes the map machine-specific and unmergeable. A position in seconds depends on the sample rate
and on the audio clock, and two machines' clocks differ; a position in host time depends on when a
particular process started, so it means nothing to a second machine at all. Worse, it breaks decision
1 from the other side: with the map in seconds and the notes in beats, a tempo change *does* move
notes, because the segments have moved while the beats have not — the exact failure this ADR exists to
prevent. Seconds are the right unit for a device's clock and for `frames_per_beat`, and the wrong unit
for the model.

**(c) A rational or PPQ-tick time base instead of `f64` beats.** ADR 0001 named this as the revisit
if rounding bites, and FL Studio's PPQ ticks are the concrete case. It is the honest alternative and
it is rejected for now on measurement rather than taste. `f64` carries about 15 to 17 significant
decimal digits, so a position near 1 200 beats (ten minutes at 120 BPM) has a resolution near
2 × 10⁻¹³ beats, while the finest grid in play — a 960 PPQ tick — is about 1.04 × 10⁻³ beats, some
nine orders of magnitude coarser. The risk with `f64` beats is therefore not resolution but
*accumulation and comparison*, and that is handled by decision 9's tolerance rather than by changing
the type. A tick or rational base would be exact by construction and would cost: every position in
the model, every command that takes a position, every importer and exporter that converts to and from
beats, every golden render and every `f64` already in the engine. That is a large, model-wide change
bought to solve a problem that measurement says does not exist yet. **The revisit condition is
concrete**: if a tick-based format's round-trip test (the `.als` and later `.flp` corpora) shows a
position that cannot be represented exactly in `f64` beats within the tolerance, this decision is
reopened in a superseding ADR. Until then the unit stays `f64` beats, deliberately and on the record.

**(d) A tempo curve rather than discrete segments.** A continuous tempo function — a ramp, an
envelope, an automation lane that is interpolated rather than stepped — is more expressive and is
rejected because it is the opposite of mergeable. Two machines that both edit a curve are in conflict
at every point of the curve, and reconciling them needs the curve's shape to be agreed; two machines
that each append a segment have a union. A curve also cannot be a state at a point (decision 2), so a
reader cannot interpret one part of it without the rest, which breaks the property that makes a
partial list readable. If a smooth accelerando is wanted, it is expressed as many small segments —
which is what a tempo automation lane would write anyway — and the model stays a step function. A
separate tempo *envelope* as a device or automation concept is not excluded by this ADR; what is
excluded is the song's tempo being one.

**(e) Derive the tempo from a played or recorded source rather than storing it.** A project could
infer its tempo map from the performance — from a tap, from the audio, from what a user played — and
store no map at all. Rejected: the tempo map is a *decision about the song*, and a decision that is
recomputed is a decision that changes when the detector improves. It also cannot express an
intention: a user who writes 128 BPM at bar 32 means it, whether or not the recording agrees. And it
would put a detector in the load path of every project, on every open, which is both slow and
nondeterministic. Detection is welcome as a *command* that proposes a map for the user to accept —
which is a different feature, and one that this decision makes possible rather than impossible.

## Not decided yet

- **The resolution of a segment's point.** Whether a point may be any `f64` beat or must be
  quantised to a grid, and to which grid (a beat, a 1/16 note, a bar). Decision 3 fixes the *unit*
  and not the *granularity*. A quantised point makes two independently authored maps easier to
  compare and to display; a free point is more expressive and is what a tempo extracted from a
  recording would produce. This is a behaviour question with a format consequence, so it is decided
  with the spec rather than here.
- **Whether a tempo map from an imported `.als` is representable.** A Live Set stores tempo and time
  signature as automation with their own defaults, which is what Suhina reads today; whether a set
  with a full tempo automation is representable in the shape decided here is not yet known, and is to
  be settled by observation against the corpus before the importer claims to support it
  ([`docs/spec/als-format.md`](../spec/als-format.md), [`docs/spec/als-corpus.md`](../spec/als-corpus.md)).
  If some of it is not representable, the importer's existing rule applies: what it does not
  understand becomes a warning and the project still opens.
- **How the map interacts with warp markers and time-stretching**
  ([ADR 0003](0003-time-stretching.md)). A warped audio clip maps its own time onto the project's,
  and the engine currently derives `frames_per_beat` once per graph; what a warped clip does when the
  tempo changes under it *within* a graph is not decided here. It is a real question — the answer
  decides whether the stretch worker is told about segment boundaries — and it belongs with ADR 0003
  rather than with the model.
- **How the tempo at a position is shown and edited in the UI**, and in particular whether the
  control bar's tempo field changes the segment in force or appends one. That is behaviour for the
  spec; this ADR only requires that changing it is an edit command with one undo step and that the
  map is never mutated by a transport command (decision 6).

## Sources

- Ableton, *Link features and functions FAQ* — the published account of a tempo change as a new
  timeline segment crossing the previous one at the point of the change:
  <https://help.ableton.com/hc/en-us/articles/209776125-Link-features-and-functions-FAQ> (read
  2026-10-15; the idea is taken from this public description, not from code, per AGENTS.md rule 1).
- [ADR 0001: A DAW-neutral project model](0001-daw-neutral-model.md) — positions are `f64` beats,
  and the stated revisit to a tick-based type this ADR answers.
- [ADR 0002: Every action is a command](0002-commands.md) — transport commands never change the
  project and record no undo step, which is what decision 6 rests on.
- [ADR 0003: Time-stretching and stretch modes](0003-time-stretching.md) — warp markers and the
  stretch worker, whose interaction with a map is left open above.
- [ADR 0005: Edits while playing](0005-edits-while-playing.md) — a new graph swapped in at a block
  boundary, carrying the playhead over on the same beat when the tempo changed.
- [ADR 0010: Suhina's own project files](0010-project-files.md) — the versioned JSON format, and the
  rule that every new model field needs its line and its round-trip test.
- Issue [#402](https://github.com/suhina-daw/suhina-daw/issues/402), "Tempo and time-signature
  changes over the song (tempo map)", label `engine` — the work this representation unblocks.
- [`docs/spec/als-format.md`](../spec/als-format.md) — tempo and time signature as they are read from
  a Live Set today, and what is not yet understood about their automation.
