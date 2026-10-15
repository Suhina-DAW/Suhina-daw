# Decide and implement musical time: a tempo map in the model, before anything depends on it

**Type:** enhancement · **Area:** `engine`, `formats` · **Phase:** earlier than the rest of the epic (one of the two deliberate exceptions: cheap now, expensive later) · **Priority:** P1

`Transport` holds a single `tempo_bpm` and one time signature today (`crates/model/src/lib.rs`), and everything that counts time derives from that one number — the launcher's beat, the stretch worker's frames per beat, the metronome, scene tempo. Every later part of [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) assumes that a position in the project means the same thing on two machines: a version diff, a shared session, a remote performer landing on the grid. If wall-clock samples or a single global tempo harden in the model first, adding a tempo map later is a model-wide rewrite. This issue decides the representation and lands it, **independently of whether any collaboration feature is ever built**. It answers [#402](https://github.com/suhina-daw/suhina-daw/issues/402).

**Depends on:** nothing — this can start today.

## Goal

A song can change tempo and time signature over its length, and every position in the model is musical time (beats), so that a tempo change moves what follows it and never what precedes it. Wall-clock time is always derived, never stored.

## Scope

1. **The governing ADR already exists:** [ADR 0024](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0024-musical-time-and-tempo.md), status **proposed**, decides the representation. This issue **implements** it — it does not write the ADR from scratch. The ADR moves to accepted with this change, or is amended with a dated note where the implementation finds it wrong (CONTRIBUTING › Issues: "architecture belongs in an ADR under `docs/decisions/`, not in an issue thread").
2. **The representation.** The epic proposes an **append-only list of segments**, each naming the beat where a tempo begins — a tempo change is a new segment, not an edit to a number. The ADR states the alternatives it rejects (a single number per scene, a list of tempo *changes* with implicit extents, storing seconds per beat, a rational/tick-based time type) and why, and says whether time signature is a second independent list or shares one.
3. **The model change.** `Transport` gains the map; `Project::validate` checks it — finite, positive, strictly increasing by beat, a segment at beat 0, sane bounds. Validation errors follow the existing `ValidationError` shape.
4. **The engine.** Everything now derived from one `frames_per_beat` (the launcher's beat and monitor, the metronome, the stretch worker's feed) reads the segment in force at the current beat. `Scene::tempo_bpm` keeps working and must not contradict the map.
5. **The file format.** A project with a tempo map is written and read back with nothing lost; a project with one tempo reads exactly as it does today (ADR 0010 decision 3: a later version keeps reading the older ones).
6. **Tests** (AGENTS.md rule 7).

## Acceptance criteria

- [ ] [ADR 0024](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0024-musical-time-and-tempo.md) is **accepted** as part of this change, or carries a dated amendment saying what the implementation found wrong with it, and `python scripts/docs.py check` passes.
- [ ] The model carries a tempo map and time-signature changes; `Project::validate` rejects a non-finite tempo, a zero or negative tempo, a segment before beat 0, two segments at the same beat, and a segment count over a stated cap — each with its own error, none of them a panic (AGENTS.md rule 2).
- [ ] Positions remain `f64` beats everywhere in `model`; no wall-clock seconds are stored in the model.
- [ ] A project with one tempo at beat 0 renders **byte-identical** to the same project before this change (the existing golden renders in `crates/engine/tests/golden/` are the oracle, and rendering is the same code path as playback per AGENTS.md rule 4).
- [ ] A tempo change part-way through a song moves the material after it and leaves the material before it where it was, shown by a golden render and by test at the exact beat of the change.
- [ ] The launcher's beat, the metronome, scene tempo and the stretch worker all agree with the map at and after a change, tested at the boundary beat and one block either side of it.
- [ ] The map survives a save and open round trip, and a `version: 1` project without a map still opens.
- [ ] A hostile map (a NaN tempo, 10⁶ segments, a segment at a negative beat, an unsorted list) is refused by name, changes nothing and does not allocate wildly.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass; `python scripts/docs.py check` passes; `python scripts/docs.py impact` is run.
- [ ] Documents updated in the same pull request: `docs/architecture.md` if any crate's role moves, `ROADMAP.md`, and `docs/commands.md` on `main` if a command changed.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- Drawing or editing the tempo map in the UI. This issue is the model, the engine and the format; the UI control is its own issue once the representation exists.
- Automation of tempo, time-signature changes imported from another format, or beat/time-signature mapping beyond what playback needs.
- Deciding anything about collaboration. This lands whether or not the rest of the epic ever does.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): a value the project must store goes in `model` first, then every reader and writer that should fill it — `crates/project-file` and `crates/als`. The engine reads it when it builds the graph; `process` must keep never allocating (rule 3), so whatever the map costs is paid in the renderer's constructor.
- **An open constraint worth settling in the ADR.** ADR 0001 › Consequences says the core's growth — "automation, tempo maps, markers, comping" — is *"checked against at least two formats before it lands, so it does not bake in one DAW's assumptions."* Only one format reader exists (`.als`); DAWproject is M7 and unstarted. So either the second format is a written spec read from public documentation, or this addition is the first to land on one format — **the ADR must say which, and why, rather than let the rule be quietly skipped.**
- **Related:** [#402](https://github.com/suhina-daw/suhina-daw/issues/402) (the tempo map this answers), ADR 0001 (positions are `f64` beats; the core grows), ADR 0003 (the stretch worker's frames per beat), ADR 0005 (the graph the tempo lives in), ADR 0010 (the file format and its versions), [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › Risks 3.
- **Decisions that belong in the ADR, not here:** the segment representation and its alternatives; whether time signature shares the structure; whether the map is capped and how; what happens at a tempo change during playback of a stretched clip; and whether a tempo map may ever be expressed in seconds.
- **The ADR number is already settled:** [ADR 0024](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0024-musical-time-and-tempo.md) exists and is proposed, and [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) is the shape it belongs to. `0016` is on `main` but not in this working tree, which is behind it — so a rebase or merge comes before the pull request, and the ADR's own citations should be re-checked after it.
- **Board:** `enhancement`; Phase earlier than the rest of the epic; Priority P1. It needs no new area label.
