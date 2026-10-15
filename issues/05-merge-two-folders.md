# Merge two project folders without git

**Type:** enhancement · **Area:** `formats` · **Phase:** M8 or later · **Priority:** P2

This is the baseline collaboration feature, and it has no networking in it: person A sends a package, person B works on it, B sends it back, and the two are merged. That asymmetry — work apart, then combine — is what musicians already do by hand, and it is the half of [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) that works with no physics to fight. Assets merge as a **union of hashes** and therefore cannot conflict; the document is the part that needs rules.

**Depends on:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (proposed) — **read it before the code: it already decides the three-way match, the conflict principle and the one-undo-step command.** Then issue 02 (object identity — a merge cannot say which clip two people edited without it) and issue 03 (the folder and the manifest). It should not be started before both.

## Goal

`project.merge` takes another project folder and produces one project: everything only one side changed is taken, everything only the other side changed is taken, and an object both sides changed is resolved by a stated rule or **reported**, never guessed at silently. One command, one undo step.

## Scope

1. **A common ancestor.** ADR 0027 decision 3 decides the merge is **three-way** and that objects are matched **by the ancestor's ids**, because the ancestor is what makes the match unambiguous. This issue implements that: a project records what it was derived from, the merge refuses with a clear message when no ancestor can be established rather than falling back to a two-way guess, and it can tell "B changed this" from "A changed this" from "both did".
2. **Matching by id.** Objects are matched by the ids issue 02 adds. Objects that exist only on one side are added; objects the ancestor had and one side removed are removed; objects both sides kept are merged field by field.
3. **Destination-allocated ids.** New objects take ids the destination project allocates, so a merge never depends on two projects having agreed on ids in advance. The allocator must be able to be told what is already taken rather than only counting up.
4. **Assets are a hash union.** The merged manifest references the union of both sides' hashes. This cannot conflict, because a content-addressed blob is immutable. An asset neither side has locally is **not** an error: the project opens in the existing supported state (`project.missingFiles`, `project.relinkFiles`, [docs/spec/missing-files.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md)).
5. **The conflict policy is decided in principle and must be implemented as decided.** ADR 0027 decision 4 is specific and this issue does not get to soften it: a field changed on one side only, or on both sides to the same value, is taken; **a field changed on both sides to different values leaves the object as the local side has it and reports it as unmerged**; deleted on one side and modified on the other is **reported**, not resolved; added on one side is imported; deleted on one side and untouched on the other is deleted. The principle is that **a merge may not silently discard a musical idea**, so a merge is **partial** — it merges what it can, declines what it cannot, and reports it by name. What remains this issue's own is the *presentation* ADR 0027 leaves to the spec: what a reported object looks like, how the user resolves one, and whether a resolution is its own command.
6. **One command, one undo step, validated.** `project.merge` is an Edit command (ADR 0002): if it produces a project that does not validate, the session is restored and the command returns an error, exactly as every other edit does.
7. **A report of what happened.** The command returns what was taken from each side, what was resolved, and what was left alone — a merge the user cannot inspect is a merge the user cannot trust.
8. Tests (AGENTS.md rule 7), including the cases that are hard to forgive: two-sided edits to one clip, a deleted-and-edited object, a merge with no ancestor, a merge of a project with itself.

## Acceptance criteria

- [ ] A merge of two folders with disjoint edits completes with no conflict, and the result equals what a person would get by applying both sets of edits by hand.
- [ ] An object both sides changed is either resolved by the documented rule or **reported in the returned result**, and the issue records which types do which.
- [ ] A merge with no establishable ancestor is refused with a message that says why, and changes nothing.
- [ ] A merge whose result does not validate restores the project exactly and returns the validation errors (ADR 0002).
- [ ] The whole merge is **one undo step**; one undo returns the project to its state before the command.
- [ ] Assets merge as a union of hashes; a merge where one side lacks assets leaves the project openable, with those assets listed by `project.missingFiles`.
- [ ] Merging a project with itself, or with an ancestor, changes nothing and is reported as such.
- [ ] Two merges in a row, in either order, of the same three projects produce the same project — or the issue records the case where they do not, and why.
- [ ] Hostile input is an error and not a crash: a folder that is not a project, a document that does not parse, a manifest whose hashes do not match, a document claiming a newer version (AGENTS.md rule 2).
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] A merge that reports unmerged objects produces a project that can be **saved, reopened and merged again later**, with the reported objects still reported. ADR 0027's consequences name this as a new kind of project state; a spec has to describe it and a test has to walk it.
- [ ] Documents updated: a new `docs/spec/project-versions.md` (versions, ancestors, the merge's behaviour and how a reported object is presented), `ROADMAP.md`, and [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) accepted as part of this change or amended with a dated note.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- git. This works with two folders on one disk or on a memory stick; the driver that lets `git merge` call it is issue 06.
- Simultaneous editing of one project by two machines at once. The epic excludes it: it needs change metadata and tombstones, and it is not needed while work happens apart.
- Any UI beyond what the command needs. The version browser is issue 07 and the musical diff is issue 08.
- Merging anything but projects — presets, plugin states or sample libraries are separate questions.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): `crates/project-file` maps the model by hand and is where the document's comparison and merge belong; `session` owns the command and its one undo step. The merge must be testable **without** a UI, a network or git: given three documents it returns one and a report. That is the shape issue 06 then wraps.
- **A merge is the hardest thing to undo, which is why the undo step is not optional.** `Session`'s undo is a snapshot of the project (ADR 0002; `crates/session/src/lib.rs`), so a merge is undone exactly as an edit is — provided it goes through the command path and does not write to disk on its own.
- **Related:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (the governing decision), [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) (the shape), [ADR 0025](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0025-object-identity.md) (the ids the match needs), [ADR 0026](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0026-project-folder-and-package.md) (the manifest the union is over), ADR 0001 (validation), ADR 0002 (one undo step, validate-or-restore), ADR 0007 (copying between open projects, listed as not decided — a merge is the bigger sibling of that), ADR 0010 (the document and its versions), [docs/spec/missing-files.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md), issue 02, issue 03, issue 06, issue 07, issue 08.
- **Already decided in [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md), not here:** git is an optional layer and the merge is the baseline; the three-way match by ancestor ids; destination-allocated ids for imported objects; assets as a union of hashes with no asset conflict; the conflict principle and the partial-with-a-report outcome; and one command with one undo step. **Still this issue's to choose, because ADR 0027 leaves it open:** what a reported object looks like and how the user resolves one (and whether a resolution is its own command); how a merge presents a change it cannot express, such as a reordering with no stable identity in the ancestor; whether a merge can be resumed or repeated after a partial result; and how much of this the CLI offers before the UI has it.
- **A question ADR 0027 leaves open, which this issue inherits:** **what a version is underneath** — a tag, a commit or a package — and whether the two storage modes share one notion of it. It matters here because the ancestor is named in those terms, so if the answer is "a named version", issue 07 has to land first and this issue's ordering changes.
- **Board:** `enhancement`; Phase M8 or later; Priority P2.
