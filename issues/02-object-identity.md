# Give clips, arrangement lanes and scenes an identity, and address commands by it

**Type:** enhancement · **Area:** `formats`, `ui`, `engine` · **Phase:** earlier than the rest of the epic (the second of the two deliberate exceptions) · **Priority:** P1

The model identifies clips, arrangement lanes and scenes **by position**, and tracks, channels and clip contents by a project-local `u32` allocated as `max + 1`. That is enough for one machine editing one project, and it is not enough for anything in [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md): a diff cannot tell a moved clip from ten changed ones, a merge cannot say which clip two people edited, and two machines both adding a track both produce `TrackId(7)`. It is also enough to produce [#352](https://github.com/suhina-daw/suhina-daw/issues/352) today, with no collaboration involved at all. **The version browser is the payoff that justifies this on its own.**

**Depends on:** nothing — this can start today. The governing decision, [ADR 0025](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0025-object-identity.md), is already written (status proposed); read it before the code, and treat its *Not decided yet* as the list of what this issue is still allowed to choose.

## Goal

Every clip, arrangement lane and scene has an id that is stable across edits, and every command and every front end names an object by its id rather than by its position on a list. The audio output does not change.

## Why this is the right first move, and what it is not

- **The engine never sorts by id.** Its sorts are by start time (`crates/engine/src/lib.rs`), chain depth (`crates/engine/src/buses.rs`), event order (`crates/engine/src/launcher.rs`), marker time (`crates/engine/src/audio.rs`) and graph sequence (`crates/engine/src/swap.rs`). So the golden renders under `crates/engine/tests/golden/`, `devices/fm-synth/tests/golden/` and `crates/ui/tests/golden/` are **not** expected to move. If one does, that is a finding, not an expected cost.
- **A command parameter type for this already exists.** `ParamType::Track` is declared as `["integer", "string"]` — "track id or name" (`crates/session/src/commands.rs`), and the JSON Schema agents read is generated from it. `ParamType::Clip` is the same shape, not a new idea.
- **This is not global or peer-scoped identity.** Ids stay project-local. Nothing here enables simultaneous editing of one project; that needs change metadata and tombstones and is explicitly out of the epic.

## Scope

Measured size of the change: **~245 id construction sites across 34 files** (`TrackId(` 111, `ChannelId(` 82, `ContentId(` 52), plus ~130 `.clips` and ~203 `.lanes` references. The type change is mechanical and the compiler finds it; **the semantic change is not, and that is the risk this issue is about.**

1. **The governing ADR already exists:** [ADR 0025](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0025-object-identity.md), status **proposed**, decides what an id is, how one is derived for a file written before it existed, and what happens to it on a copy. This issue **implements** it, and the ADR moves to accepted with this change or is amended where the implementation finds it wrong.
2. **(A) Ids in the model.** `Clip`, `ArrangementLane` and `Scene` gain an id. Existing ids keep their type. **A `version: 1` file has none, so the reader derives them in document order** — lane order, clip order within a lane, scene order — deterministically, so the same v1 file always yields the same ids.
3. **(B) All addressing moves to ids**, UI included: `selected_clip()` returns an id rather than `(TrackId, usize)` (`crates/ui/src/arrangement_view.rs`), the clip context menu takes an id (`crates/ui/src/menus.rs`), copy/cut/paste and the sub-row layout in `crates/ui/src/timeline.rs` follow.
4. **(C) Command parameters.** A `ParamType::Clip` alongside `ParamType::Track`, and the **seven** `clip.*` commands that take a positional clip `index` (`clip.delete`, `clip.setGain`, `clip.setFades`, `clip.copy`, `clip.cut`, `clip.duplicate`, `clip.move`; `track.move`, `device` and `scene` take positions of their own) accept an id — **and keep accepting `index` for a deprecation window**, documented in each command's own description, because command parameters are part of the public interface for agents (ADR 0002).
5. **(D) Read commands report the id** as well as the index, so an agent can read a project, act on it, and not have its target move underneath it.
6. **Validation errors become nameable.** `ValidationError::BadFades { lane, clip }` and its neighbours report indices today; they gain the id so a message can name the object the user sees.
7. Tests (AGENTS.md rule 7), including the one that would have caught #352.

## Acceptance criteria

- [ ] [ADR 0025](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0025-object-identity.md) is **accepted** as part of this change, or carries a dated amendment; `python scripts/docs.py check` passes.
- [ ] Moving one clip changes only that clip's record: a test that compares two projects before and after a move shows exactly one clip different.
- [ ] Splitting a clip produces two clips whose ids are new and whose neighbours keep theirs; copying a clip produces a new `Clip` id.
- [ ] **A duplicated clip can be edited without changing its sibling** ([#352](https://github.com/suhina-daw/suhina-daw/issues/352)), with a test that fails today.
- [ ] Reading the same `version: 1` file twice yields identical ids, and reading two files that differ only in a clip's position yields the same ids for the clips that did not move.
- [ ] Each of the seven clip commands (`clip.delete`, `clip.setGain`, `clip.setFades`, `clip.copy`, `clip.cut`, `clip.duplicate`, `clip.move`) accepts an id and still accepts the old `index`, and `commands.list` shows both in the parameter description.
- [ ] `commands.list` and `docs/commands.md` reflect the new parameter type; `keymap.json` bindings that name a clip command keep working.
- [ ] The golden renders under `crates/engine/tests/golden/`, `devices/fm-synth/tests/golden/` and `crates/ui/tests/golden/` are unchanged; if any changes, the issue records why before it lands.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] The tree builds at every commit (AGENTS.md › Before you finish a task, "Keep the tree building at every step"): steps A–D land as separate pull requests, not one.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- Global, peer-scoped or UUID identity; change metadata; tombstones; a CRDT. The epic deliberately excludes them while one host holds the truth.
- The project folder, the manifest and the package (issue 03), the merge (issue 05), the version browser (issue 07) and the musical diff (issue 08), which all depend on this.
- Relabelling the UI. A user never needs to see an id.

## Notes

- **Where the code goes:** `model` first — the ids and the derivation — then every reader and writer (`crates/project-file`, `crates/als`), then `session` for allocation and the command parameters, then `ui` for the addressing.
- **Id allocation must stop being `max + 1`** if two files are ever merged: a merge that renumbers is acceptable (issue 05 allocates ids at the destination), but the allocator has to be able to be told what is free rather than only counting up.
- **`crates/session/src/blank.rs` uses literal ids** (`ChannelId(90)`, `ChannelId(91)`, `ChannelId(100)`, `TrackId(10 + offset)`), which is fine while ids are project-local and must be revisited if they ever are not.
- **The deprecation window needs an end.** The ADR says how long `index` is accepted and what removes it; `index` cannot be accepted forever, or both meanings stay live and the semantic ambiguity this issue exists to remove comes back.
- **Related:** [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) (the shape this belongs to), [ADR 0025](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0025-object-identity.md) (the governing decision), [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (which depends on this for its three-way match), [#352](https://github.com/suhina-daw/suhina-daw/issues/352), ADR 0001 (the model and its id types), ADR 0002 (command parameters are the agent interface), ADR 0007 (copying between open projects, listed as not decided — it needs this), [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › "The prerequisite that is not networking", issues 05, 07 and 08.
- **Decisions that belong in the ADR, not here:** what an id is made of; how it is derived for a file written before it existed; whether an id survives a copy, and if not, what the copy semantics are; how long `index` is accepted; and whether the allocator is asked for a free id or infers one.
- **A question to resolve:** whether ids for lanes and scenes are in scope now or can follow, given that a diff over clips is what issue 08 needs first. Splitting this issue is preferable to a vague scope; if it is split, say so here rather than leaving it implicit.
- **Board:** `enhancement`; Phase earlier than the rest of the epic; Priority P1.
