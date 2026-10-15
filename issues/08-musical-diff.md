# A musical diff: which tracks changed between two versions, and play them

**Type:** enhancement · **Area:** `ui`, `engine` · **Phase:** M8 or later · **Priority:** P2

Git can say which lines changed. Only a DAW can **play** them. This is the genuinely new part of [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) and the reason the whole versioning half is worth building: given two versions of a project, say which **tracks** differ and let the user hear the difference — side by side or one after the other. It is also the feature that justifies object identity (issue 02) on its own, with no collaborators anywhere.

**Depends on:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (proposed) — **decision 8 decides this diff and decision 9 decides why auditioning is cheap; read both before the code.** Then issue 02 — **without object identity this feature is noise.** Moving one clip currently makes everything after it on the lane look changed, and splitting a clip makes half a lane look changed. It also needs issue 07 to present the two versions, though the diff itself is a command and can be built and tested before the panel exists.

## Goal

Given two versions of a project, report what changed per track in terms a musician recognises — this track gained a clip, this one's notes changed, this device's setting moved, this fader moved — and audition the changed material without leaving the project.

## Scope

1. **The diff itself, as a command.** `project.diff { a, b }` returns a structured result: per track, what changed and how much; per clip, added, removed, moved, or edited; per device, which parameters moved. This is a read command (ADR 0002), needs no undo, and must be testable with no UI. **The definition of "changed" per object type is the decision this issue must not invent** — it belongs in the spec, written before the code.
2. **Object granularity first, audio second.** The diff compares *the model*, not rendered audio. Two reasons, both already established: the engine never sorts by id, so a structural comparison is cheap and exact; and a bit-identical render is **not** guaranteed across machines or even across runs with plug-ins, so a diff defined on rendered audio would be unreliable by construction (AGENTS.md rule 4 keeps the render as the *engine's* oracle, not as a comparison key).
3. **Audition.** At least two shapes, and the spec says which is which:
   - **In sequence:** play version A from a point, then the same point of version B. This is one graph swap with the playhead carried, which the engine already does at a block boundary (ADR 0005) — so it works **while playing**, and it is the same mechanism a version switch uses (issue 07).
   - **Changed tracks only:** play just the tracks that differ, or solo them, which the mixer's solo already reaches.
   - A side-by-side of two *rendered* files is a third option and belongs to `project_render`; if it is offered, say that it renders and is therefore slower and not bit-comparable across machines.
4. **A readable summary**, not a log: tracks that changed, in the order the project shows them, with the change in a few words. What a user does with it is decide what to keep.
5. **The cases that are not "changed":** tempo and time-signature changes (they move everything and must be reported as such rather than as every clip moving), a version whose audio or plug-ins are absent, and two versions that are identical.
6. Tests (AGENTS.md rule 7), including the noise cases that issue 02 fixes — move a clip, split a clip, reorder two clips — each of which must report **exactly one** change.

## Acceptance criteria

- [ ] `project.diff { a, b }` returns a structured, documented result; it is a read command, it changes nothing, and it is tested with no UI.
- [ ] Moving one clip reports one changed clip, not the lane after it; splitting a clip reports a removal and two additions; reordering two clips reports two moves. **Each of these tests fails before issue 02**, which is the point.
- [ ] A tempo change is reported as a tempo change, not as every clip having moved.
- [ ] A device parameter change is reported per device and per parameter, and a fader or pan change is reported per track.
- [ ] Auditioning A then B while playing continues from the same beat, with no gap, using the graph swap the engine already performs.
- [ ] Identical versions report no changes, and that is a normal result, not an error.
- [ ] A version whose audio or plug-ins are absent produces the structural diff and says what it could not compare, rather than failing.
- [ ] The result is deterministic: the same two versions diffed twice give the same result.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: `docs/spec/project-versions.md` (the diff's definition and audition behaviour), `docs/commands.md` on `main`, `ROADMAP.md`, and `docs/design/components.md` if a new component appears.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **Merging.** Reporting a difference and resolving it are different features; the merge is issue 05.
- Editing from the diff, taking one side, or a three-way view. If "take theirs for this track" is wanted, that is a follow-up built on the merge.
- A textual or line-level diff. The user sees tracks, not JSON.
- Comparing rendered audio bit for bit. See above: it is not reliable, and the epic says so.
- Automation, comping or a change history over time.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): the comparison belongs beside the document's other work in `crates/project-file` (it maps the model by hand and knows the fields), or in `session` if it needs two open projects at once — that choice should be made deliberately, because ADR 0007's "copying clips, tracks and devices between open projects" is the same problem and is still listed as not decided. The audition is `ui` over existing transport commands.
- **The audition is cheaper than it sounds, and that is worth stating in the spec.** Comparing two versions while playing is not "load the other project"; it is building the other graph and swapping to it at a block boundary with the playhead carried, which ADR 0005 already does for edits, and with plug-in host processes carried over when the chain is unchanged. Issue 07 leans on the same property.
- **Related:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (the governing decision — 8 and 9), [ADR 0025](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0025-object-identity.md) (the identity without which this is noise), [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) (the shape), ADR 0001 (the model a diff walks), ADR 0002 (read commands and the agent interface), ADR 0005 (the swap that makes A/B work while playing), ADR 0006 (why a render is not a comparison key), ADR 0007 (comparing two open projects), ADR 0024 (the time representation a diff compares), issue 02 (the prerequisite), issue 07 (the list this sits in), issue 05 (the merge that acts on a difference), [docs/spec/missing-files.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md).
- **A cost ADR 0027 records, which belongs in this issue's own words:** *"the document's heaviest part is plug-in state, not audio, so the cost of a diff is usually plug-in state"* — because a plug-in's state is stored base64 in the document (ADR 0010 §5). A diff over a project with large sampler states is therefore not free, and the issue should say so rather than promise an instant comparison.
- **Decisions that belong in a spec or ADR, not here:** what counts as a change per object type; whether a move is one change or two; how a tempo change is reported; what the audition plays by default; and whether the diff is computed on demand or cached per version pair.
- **A question to resolve:** whether the diff is computed between two **open** projects (which needs ADR 0007's decision about several projects at once) or between the open project and a version read from disk. The second is simpler and is what the version browser offers; the first is what a "compare my tab with theirs" workflow would need. Pick one and say so.
- **Board:** `enhancement`; Phase M8 or later; Priority P2.
