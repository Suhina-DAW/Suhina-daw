# A version browser: named versions a musician can read, hear and return to

**Type:** enhancement · **Area:** `ui`, `formats` · **Phase:** M8 or later · **Priority:** P2

The workflow this formalises already exists and is clumsy. Measured in one authored project folder: **five named sets** (`Trance Test.als`, `Trance Test v2026.als`, `Trance Test v2026#2.als`, `Nice Sounds.als`, `KEKW.als`) and **five timestamped copies** in a backup folder — 38 % of the folder's bytes, because each copy is a whole document that does not share anything with its siblings. Musicians are doing version control by hand with "Save As". This issue gives them the list. It is deliberately **not** a commit graph: no nodes, no edges, no hashes.

**Depends on:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (proposed) — **decision 7 decides this browser and decision 9 decides why a switch is cheap; read both before the code.** Then issue 03 (the folder and the package) for the package mode, and issue 06 (git) for the repository mode. **The package mode can land without git and should** — it is the mode most users will have.

## Goal

One list of the project's versions, whichever way they are stored: name, when, who, and how long the song was. Selecting one opens it. Returning to the previous one costs a graph swap, not a reload.

## Scope

1. **What a version is, for the user.** ADR 0027 decision 7 decides the browser lists versions with **date, author and length**, works over **both** storage modes — packages in a folder and commits in a repository — and that what the user chooses is a **version**, never a storage mechanism, so a user who never learns what git is still gets versions. This issue implements that. What the ADR explicitly leaves open, and what this issue must settle, is **what a version is underneath** (a tag, a commit or a package) and whether the two modes share one notion of it.
2. **The list.** Name, date, author and length. Length is the arrangement's, which the engine already computes (`Renderer::length`, `crates/engine/src/lib.rs`); a version whose audio is absent shows that rather than a wrong number.
3. **Opening a version** is one command in the `project.*` family, and it must be cheap by construction:
   - reuse the decoded audio pool;
   - call `set_audio` **only when the set of referenced hashes changed** (`audio_changed` and `Controller::set_audio`, ADR 0011) — a version switch that re-decodes identical audio is a bug, not a slow path;
   - **keep plug-in host processes alive** when the device chain is unchanged; a version switch is a graph swap at a block boundary (ADR 0005), not a restart;
   - carry the transport where it was, as an output change already does.
4. **Unsaved changes.** Switching with unsaved changes asks, as closing a project already does (ADR 0010 decision 9), and the answer is remembered for the rest of the session in the same way.
5. **Deleting and renaming a version**, with the deletion of a package or a commit being explicit about what it removes from disk.
6. **The absence cases:** no versions yet; a folder that is not a repository; no git installed; a version whose document does not parse (a corrupted file is listed and refuses to open, rather than being hidden).
7. Tests (AGENTS.md rule 7), including that a version switch during playback does not re-decode and does not restart a plug-in.

## Acceptance criteria

- [ ] The version list shows name, date, author and length for packages in a folder, and for commits when the folder is a repository, in one UI.
- [ ] Selecting a version opens it; the project's own file and its unsaved marker follow the rules ADR 0010 decision 9 already sets.
- [ ] **Switching to a version whose referenced hashes are unchanged does not re-decode audio**, shown by a test that counts decodes (or by the pool's own instrumentation), not by a stopwatch.
- [ ] **Switching to a version whose device chain is unchanged does not restart a plug-in host process**, shown by the process's identity being unchanged across the switch.
- [ ] Switching while playing continues from the same beat and does not glitch the transport, as an output change already behaves.
- [ ] A version whose audio is absent lists its length as unknown or partial rather than a wrong number, and opens in the missing-files state.
- [ ] With no git installed, the package mode is complete and the repository mode says why it is unavailable (ADR 0015 §2).
- [ ] A version whose document does not parse is listed and refuses to open with a message; it is not silently dropped from the list.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: a new `docs/spec/project-versions.md` (or that spec's UI section), `docs/shortcuts.md` if anything becomes a key, `ROADMAP.md`, the UI component measurements in `docs/design/components.md` if a new component is introduced, and [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) accepted as part of this change or amended with a dated note.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **A graph.** No nodes, edges, merge commits or hashes in the UI. If a user wants git's own view, they have git.
- The merge itself (issue 05) and the driver (issue 06). This lists versions; combining them is that issue's job.
- The musical diff (issue 08) — knowing what changed between two versions is a separate feature on top of this list.
- Autosave and automatic versioning ([#202](https://github.com/suhina-daw/suhina-daw/issues/202)). Whether a version is created automatically is that decision, not this issue.
- Syncing, uploading, or a remote.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): the listing and the switch are `session` commands over `Projects` (the projects kind, since a version is a whole project); the panel is `ui`, drawn from the list the command returns and dispatching commands, never editing the model. The UI reads no git and no zip itself.
- **The measured argument for this issue is in the epic:** ten near-identical documents were 15 MB of a 39 MB folder, and a repository would hold the same ten for an estimated 2–3 MB, because they are near-identical text. **So the list is not only a convenience — it is where the space is saved.** Say so in the issue's own words, because "version browser" sounds like a nicety and this one is not.
- **Related:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (the governing decision — 7 and 9 in particular), [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) (the shape), [ADR 0026](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0026-project-folder-and-package.md) (what a package is), ADR 0005 (the graph swap the switch reuses), ADR 0007 (several open projects — a version is opened into a tab or replaces one, and that choice should match what is already decided there), ADR 0010 (saving, unsaved changes, the file the project came from), ADR 0011 (`set_audio`), issue 03, issue 05, issue 06, issue 08, [#202](https://github.com/suhina-daw/suhina-daw/issues/202).
- **One honest limit ADR 0027 records and this issue inherits:** a plug-in's **state** lives in the document as base64 (ADR 0010 §5), so switching between two versions that differ in a plug-in's state pushes new state to its host — a `setState`, not a restart, but not free. And **the document's heaviest part is plug-in state, not audio**, so the cost of comparing or switching versions is usually plug-in state. Do not promise that a version switch is instant.
- **Decisions that belong in an ADR or the spec, not here:** what a version is underneath and whether the two modes share one notion of it; whether opening a version replaces the tab or opens a new one; what deleting a version removes from disk; and whether the list is a panel, a dialog or a column.
- **Where "author" comes from is resolved, and this issue no longer owns it.** It is **issue 15**: a display name in the settings, the project's author (its
  creator, inherited by a fork), and a **per-version author** defaulting to the one it descends from. ADR 0027
  decision 7's author column reads the per-version one, which is the only shape that stays true once somebody else
  contributes. This issue shows the column; issue 15 fills it.
- **One question still open in this issue:**
  1. **Whether a version needs a name before it can exist.** If a version is created by naming it, the list is always tidy; if versions appear automatically, the list needs the naming-free path too. This changes what the user does, so it belongs in the spec.
- **Board:** `enhancement`; Phase M8 or later; Priority P2.
