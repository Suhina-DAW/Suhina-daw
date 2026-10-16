# Remove the `.locr` compatibility path

**Type:** enhancement · **Area:** `formats`, `ui` · **Phase:** M8 or later, but **unblocked and worth pulling forward — it must land before issue 03** · **Priority:** P1

Files saved before the product was renamed were written with a different extension and a different `"format"` string, and they are still read and saved back to the same path ([ADR 0010](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0010-project-files.md) decision 10). It has to go, for one reason above the others: **the extension is a fossil of the product's earlier name, which this project is not permitted to write out** — CI runs `python scripts/rename_suhina.py --check`, and the old name is banned from the tree. A compatibility path whose own name cannot be explained in the code is a path that will confuse every future reader. It is also cheap and has no users: **no file of that kind exists anywhere in this repository**, and the epic records that no real project file of any kind has been saved yet.

**Depends on:** nothing — this can start today. It must land before issue 03, which decides the package's extension and should settle the naming once.

## Goal

The old extension and the old `"format"` string are gone from the code, the tests and the user-facing text; ADR 0010 records the removal as a decision with its reason; CI still passes.

## Scope

The measured size of the change: **about 39 occurrences across 12 files**, of which the **functional code is two constants and two branch points**. The rest are tests, doc comments and strings a user reads.

1. **First, check the rename markers.** `python scripts/rename_suhina.py --check` runs in CI and allows lines marked `rename_suhina: keep`. **Before anything is deleted, check whether the lines that mention the old extension carry that marker.** If they do, the removal also removes an exception, and the check must still pass afterwards. If they do not, the check passes today only because the pattern does not match, and it must still pass afterwards. Either way the answer is a finding, not an assumption.
2. **The functional removal**, in `crates/project-file/src/lib.rs`: the old extension constant, the old format constant, the branch in `is_project_file`, and the branch that accepts the old format string when reading.
3. **The tests** that assert the old behaviour are removed rather than left asserting something that is now impossible — but the tests that assert a **newer version is refused** stay, because that promise (ADR 0010 decision 3) is unaffected.
4. **The user-facing text**: the CLI's help, the app's help and drop-target text, the recent-files list, the project dialog, and the MCP tool description for opening a project.
5. **ADR 0010 gains a decision 11** recording the removal, dated, with the reason: no file of that kind exists, the name cannot be written out, and it is cheaper to remove it now than to keep three format names alive through the version bump issue 03 brings.
6. **`docs/spec/suhina-format.md`** is updated, since it is the document that describes what a reader accepts.

## Acceptance criteria

- [ ] `python scripts/rename_suhina.py --check` passes, and whether a `rename_suhina: keep` exception was removed is stated in the pull request.
- [ ] The old extension and the old `"format"` string appear nowhere in the tree: `crates/`, `apps/`, `docs/` (except the ADR that records the decision), `README.md` and `ROADMAP.md`.
- [ ] A document naming the old format string is refused the same way any unknown format is, with a message that names what was found.
- [ ] A document naming a **newer** version than this build reads is still refused, with the existing message (ADR 0010 decision 3) — this must not be lost in the removal.
- [ ] `is_project_file` accepts the current extension and nothing else, tested with the old extension, the current one, and neither.
- [ ] No user-facing text mentions the old extension: `apps/suhina-cli/src/main.rs`, `apps/suhina/src/main.rs`, `apps/suhina/src/recent.rs`, `crates/ui/src/project_dialog.rs`, and the project-open description in `crates/automation/src/server.rs`.
- [ ] The round-trip and format tests in `crates/project-file/src/tests.rs` still cover everything except the removed path.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated in the same pull request: ADR 0010 (decision 11), `docs/spec/suhina-format.md`, `ROADMAP.md` › What works now, and `README.md` if it mentions it.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **A migration tool.** If a file of that kind did exist, the answer would be to open it once and save it as a current one — not to keep the reader alive. This issue assumes none exists; if one is found, say so in the issue before deleting rather than after.
- The format version bump itself, which is issue 02 and issue 03.
- Renaming anything else, or revisiting the current extension.

## Notes

- **The files that carry it** (about 39 occurrences in total): `crates/project-file/src/lib.rs` (the two constants and the two branch points, plus doc comments), `crates/project-file/src/tests.rs`, `crates/session/src/lib.rs`, `crates/session/tests/save.rs`, `crates/session/tests/relink.rs`, `crates/automation/src/server.rs`, `crates/media/src/info.rs`, `crates/ui/src/app.rs`, `crates/ui/src/project_dialog.rs`, `apps/suhina-cli/src/main.rs`, `apps/suhina/src/main.rs`, `apps/suhina/src/recent.rs`, and `docs/spec/suhina-format.md`. Grep the tree for the remaining references — `ROADMAP.md` and `docs/decisions/0010-project-files.md` carry some, and `docs/INDEX.md` is generated so it follows on `main`.
- **Where the code goes:** `crates/project-file` owns what a project file is; the text changes are wherever the string is shown. **Neither `model`, `engine` nor the audio path is involved**, which is what makes this the safest item in the epic to start with today.
- **Related:** ADR 0010 (decisions 3, 10 and the new 11), [docs/spec/suhina-format.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/suhina-format.md), issue 03 (which decides the package's extension and should not have to think about three names), [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › Sub-issues (9).
- **A decision to make in this issue, not in an ADR:** what a user sees if a file of that kind is dropped on the window or typed into the open dialog. Silently ignoring it is the simplest and the least kind; **one clear message saying it was saved by an older Suhina and is no longer read** costs a line and avoids a file seeming to vanish. This issue proposes the message and asks for confirmation, because it is a behaviour, not a code detail.
- **Board:** `enhancement`; Phase M8 or later; Priority P1, on the reasoning that this is the only item in the epic that is both completely unblocked and *removes* work, and that leaving it later costs a naming decision in issue 03 that would then have to be made twice.
