# Let real git merge a project: a merge driver, gitignore rules, and keeping `.git` out of the library

**Type:** enhancement · **Area:** `formats`, `browser` · **Phase:** M8 or later · **Priority:** P2

A `.suhina` document is JSON that "people and agents can read and diff" ([ADR 0010](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0010-project-files.md)) — which makes it **text that is not line-oriented**, and `git merge` on it produces mangled JSON and conflicts in the wrong places. Git already has a supported answer for exactly this: a **merge driver**, the same mechanism used for lock files and generated project files elsewhere. This issue wraps the merge from issue 05 in one, states the rules that keep audio out of git, and fixes a real interaction between a repository and the library's scanner.

**Depends on:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (proposed) — its decision 6 already decides that the driver exists, that the repository carries `*.suhina merge=suhina` in `.gitattributes`, and that the driver is **not optional for the git path** because a textual merge mangles the document. Read it first. Then issue 05 (the merge it drives) and issue 03 (the folder whose `.gitignore` this writes). The library's scan and watch fix stands alone and could land first.

## Goal

A project folder can be a git repository, `git merge` on two diverged documents produces a valid project instead of a conflicted file, audio never enters the repository, and a repository inside a folder the browser watches does not poison the index or the watcher.

## Scope

1. **`suhina merge-driver %O %A %B`** as a subcommand of `suhina-cli`, beside the other non-UI commands. It calls the same merge as `project.merge`, writes the result to `%A`, exits 0 on a clean merge and non-zero on a conflict — and it **must not need a UI, a session or a network**. The merge itself stays in `crates/project-file`; the binary is a thin wrapper, so the same code is tested once.
2. **What it writes.** `.gitattributes` with `*.suhina merge=suhina` (and the package's extension, from issue 03), plus the `merge.suhina.driver` configuration, written when a project folder becomes a repository — and **explained before it is written**, because it changes how the user's git behaves.
3. **`.gitignore` rules** that keep audio and renders out: the collected `samples/` folder and the render output. Committing unchanged audio does not duplicate it, but **every new take or render is a permanent blob and removing it means rewriting history** — which is why the audio never goes in, while the manifest does.
4. **The library must not scan or watch `.git`.** This is measured, and it is two problems rather than one:
   - **Not a correctness problem.** Files are indexed only when `kind_of` recognises them (`crates/library/src/index.rs`), so `.git/objects/…` and `.git/HEAD` are never indexed, and search is not polluted.
   - **A real cost.** The scanner **recurses into every sub-directory** whatever its name, so it walks the whole `.git` tree; and ADR 0012 §5 budgets a recursive watch as **one inotify watch per sub-folder**, against half of `fs.inotify.max_user_watches`. `.git/objects/ab/` is a lot of sub-folders, so a repository inside a watched library folder eats that budget and can push the folder onto polling. Every git operation then fires the watcher.
   - The fix is a skip-list for dot-directories (`.git` and its relatives), which is worth having regardless of this epic.
5. **Absence is an ordinary state.** With no `git` on the machine, everything else works and the version features degrade to the package path (issue 03) with a message — the shape ADR 0015 §2 requires. The same for a folder that is not a repository.
6. **A decision about cloud-synced folders.** A git repository inside a cloud-synced folder is a known source of corruption (partial writes, conflict copies of pack files). The epic lists it as a risk; this issue is where it is decided and, if the decision is to warn, where the warning lives.
7. Tests (AGENTS.md rule 7).

## Acceptance criteria

- [ ] `suhina merge-driver` on three files (ancestor, ours, theirs) writes a valid project to the "ours" path and exits 0 when the merge is clean; on a conflict it exits non-zero, says what conflicted, and **leaves a document that still parses** — a driver that writes half a project is worse than one that refuses.
- [ ] `git merge` on two branches whose documents diverged produces a project that opens, with no textual conflict markers in the file.
- [ ] A repository created for a project has `.gitattributes` and `.gitignore` that keep `samples/` and render output out; `git status` after a render shows no new audio.
- [ ] The document and the manifest **are** tracked, so a version is diffable in the repository.
- [ ] A repository inside a watched library folder adds nothing to the index (tested), and the watcher's budget is not spent on it (tested on Linux where the budget applies, or written down as a manual check if it cannot be automated).
- [ ] With no `git` binary on the `PATH`, every other feature works and the version features report the missing capability with a message (ADR 0015 §2), with a test.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: `docs/spec/project-versions.md` (the git path), `SECURITY.md` only if the decision changes its scope, `docs/architecture.md` if a binary is added, and `docs/shortcuts.md` if anything becomes a key.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- A git GUI, a branch graph, or an in-app commit history. The version browser is issue 07 and it is deliberately **not** a commit graph.
- Hosting, remotes, authentication, or anything the project operates. `git push` is the user's own git and the user's own account.
- Replacing git. The merge the driver calls must work with no git present at all (issue 05).
- Autosave ([#202](https://github.com/suhina-daw/suhina-daw/issues/202)). If autosave ever writes files, they go in `.gitignore` — that is the most this issue does about it.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): a subcommand in `apps/suhina-cli`, the merge in `crates/project-file`, the skip-list in `crates/library`. **Nothing in `engine`, `model` or the audio path is involved**, which is what makes this issue safe to do in parallel with the rest.
- **The driver's contract is the deliverable, not the plumbing.** Git calls it with three paths and reads the exit code; the value is in *what the merge does*, which is issue 05. Write the contract down in `docs/spec/project-versions.md` so a third party can implement a driver too.
- **Related:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) (the governing decision, and decision 6 in particular), [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) (the shape), ADR 0012 (the index and its watch budget — the constraint this issue works inside), ADR 0015 (why absence must be an ordinary state, and the "separate program" pattern git itself fits), ADR 0010 (the document and its versions), issue 03, issue 05, issue 07, [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › Risks 6.
- **A question ADR 0027 explicitly leaves open, and the sharpest one here:** *"Where the merge driver's configuration is written. A repository needs `git config` for the driver; whether Suhina writes it, offers it, or documents it is open, and **writing into a user's git configuration unasked is not acceptable**."* This issue must answer that before it writes anything into a user's repository, and the answer is the difference between a helpful feature and a rude one.
- **Decisions that belong in an ADR or the spec, not here:** whether Suhina offers to make a folder a repository or leaves it to the user; whether git is a supported configuration or merely tolerated; what is written into `.gitattributes` and `.gitignore` and whether the user may edit it; and what is done, if anything, about a repository in a cloud-synced folder.
- **A question to resolve:** whether a project folder should be made a repository **by default**. This issue assumes not — a repository is the user's choice, and most sharing is a package — but if the answer is yes, it interacts with issue 07 and with the cloud-sync risk.
- **Board:** `enhancement`; Phase M8 or later; Priority P2. The library skip-list alone is small enough to be its own `good first issue` if this issue is split.
