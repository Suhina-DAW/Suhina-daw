# A project is a folder: document, manifest and assets

**Type:** enhancement · **Area:** `formats`, `ui` · **Phase:** M8 or later · **Priority:** P2

ADR 0010 decision 6 says audio is *referred to, not copied*, and its *Not decided yet* list carries "**a packaged format that carries its audio, and 'collect and save'**" plus "backups and autosave". [ADR 0026](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0026-project-folder-and-package.md) (proposed) decides it: a project folder holding the document, a **manifest** mapping each asset to its path and its BLAKE3 hash, and the audio itself when the user asks for it. This issue implements that decision. It is the prerequisite of everything in [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) that moves a project between machines, and it is also what makes versioning worth anything: a document that can be diffed is only useful if the thing it points at can be found and verified.

**Depends on:** issue 02 — the two share a file-format version bump, and landing them in a stated order is cheaper than bumping twice. It also shares one naming decision with issue 09: the package's extension and the retirement of the old one should be settled together.

## Goal

A project is a folder. The document and a manifest live in it, the audio it references may live in it, and the whole folder can be copied, sent, versioned or merged, with every asset verifiable by its own bytes rather than by where it happens to sit.

## Scope

1. **The governing ADR already exists:** [ADR 0026](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0026-project-folder-and-package.md), status **proposed** — the folder layout, the manifest's format, the package's extension, and what is stored versus referenced. This issue **implements** it, and the ADR moves to accepted with this change or is amended where the implementation finds it wrong.
2. **The manifest.** Per asset: its path relative to the folder, an absolute path for when that does not resolve (the pair ADR 0010 already keeps per region), and **its BLAKE3 hash**. One structure serving reference, collecting, relinking and merging.
3. **Reference by default; collecting is an explicit action.** Opening a project changes nothing on disk. Opening a file that lives elsewhere keeps working. Collecting copies what the project references into the folder, **skips an asset whose hash is already present**, and is one undo step.
4. **The package.** A zip (ADR 0010 decision 6 already names the shape) written and read back with nothing lost. `zip` is already a workspace dependency, used for theme backups and exports. **The package gets an extension of its own so `is_project_file` stays unambiguous** (`crates/project-file/src/lib.rs`).
5. **The format version bump**, keeping the reader's promise that a later version still reads the older ones (ADR 0010 decision 3), with the version the reader refuses moved in `crates/project-file/src/tests.rs` rather than deleted.
6. **Hostile input.** The existing read cap is 512 MiB per file (ADR 0010 decision 4); a package needs its own cap and its own refusals — an entry whose path escapes the folder, an absolute or `..` entry, an entry count over a cap, a declared size that does not match, a zip bomb.
7. **Path length.** Measured on an authored project: one collected stem's filename was ~120 characters and its full path ~170. A project under a deep user folder exceeds 260 on Windows. The ADR says what is done about it (short generated names, a warning, or refusing to place) rather than leaving it to fail on one platform.
8. Tests (AGENTS.md rule 7).

## Acceptance criteria

- [ ] [ADR 0026](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0026-project-folder-and-package.md) is **accepted** as part of this change, or carries a dated amendment; `python scripts/docs.py check` passes.
- [ ] A project saves and reopens as a folder with a document and a manifest, and **a `version: 1` `.suhina` file still opens** ([ADR 0010](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0010-project-files.md) decision 3).
- [ ] A manifest records a path and a BLAKE3 hash per asset; a file whose bytes do not match its recorded hash is reported **by name**, and nothing is written to the project or to disk.
- [ ] Collecting copies what the project references, leaves an asset already present by hash alone, and is one undo step; the whole project one undo step as it was before.
- [ ] A package written and read again reproduces the project exactly (round trip), and `suhina_project_file::is_project_file` distinguishes a document from a package.
- [ ] A project whose assets are absent opens in the **existing supported state** — `project.missingFiles` and `project.relinkFiles` (`crates/session/src/relink.rs`, [docs/spec/missing-files.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md)) — and the offline render of that project is a golden file, so the degraded case is tested rather than discovered.
- [ ] Hostile packages are refused by name: an entry that escapes the folder, an absolute path, more entries than the cap, a size mismatch, and a decompression bomb. None of them panics or allocates without bound (AGENTS.md rule 2).
- [ ] A path that would exceed the platform's limit is handled as the ADR states, with a test.
- [ ] Every new field has its line in `crates/project-file` and in the round-trip test (ADR 0010 › Consequences).
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: `docs/spec/suhina-format.md`, a new `docs/spec/project-package.md`, `ROADMAP.md`, and a row in `ATTRIBUTION.md` for `blake3` (CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception).

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- The merge over two folders (issue 05) and its driver (issue 06).
- Autosave and backups ([#202](https://github.com/suhina-daw/suhina-daw/issues/202)) — a separate decision, and the measured cost of doing it the obvious way is in the epic: five timestamped copies of one document were 21 % of a 39 MB project folder.
- Content addressing for anything but assets: the hand-set kind store is issue 04, and the derived analysis caches stay where ADR 0013 puts them.
- Uploading anywhere, and any service the project operates.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): `crates/project-file` owns the folder, the manifest and the package — it already maps the model to JSON by hand and it already decides what a project file is. `session` owns the commands (`project.save` grows, and a collect command) and holds the decoded audio; `ui` gets the menu items and the progress. Nothing below the UI may depend on a network library.
- **Where the audio goes, and the one question the ADR must answer:** whether an asset is hashed as its **file bytes** or as its **decoded samples**. File bytes are cheap and are what is transferred; decoded samples are what the engine actually plays. The epic assumes file bytes; the ADR should say so and say what happens when the same audio exists in two containers.
- **Readability costs size.** A plain-JSON document is larger on disk than a compressed one, which is the measured consequence of ADR 0010's choice — see the epic's measurements. The package is where that is paid back, and it is worth stating in the ADR that the package is compressed and the loose document is not.
- **Related:** ADR 0010 (the decision this completes), ADR 0011 (`set_audio`, which is what the engine is told when the referenced set changes), ADR 0012 and ADR 0013 (where caches live, and why this one does not), [docs/spec/missing-files.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md), issue 04, issue 05, issue 06, issue 09.
- **Decisions that belong in the ADR, not here:** the folder layout; the manifest's format; the package's extension and whether the document inside is the same file; the caps; whether collecting may write outside the project folder; and what is hashed.
- **A question to resolve:** whether the folder is the only shape, or whether a single loose document stays a first-class project. The epic assumes both exist — the folder is the working shape and the package is the exchange shape — but the ADR should say plainly whether a loose document is a project of its own or only an intermediate.
- **Board:** `enhancement`; Phase M8 or later; Priority P2.
