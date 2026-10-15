# An author identity: a name for a person, a key for a peer

**Type:** enhancement · **Area:** `formats`, `ui` · **Phase:** earlier than the rest of the epic — it is small, and issues 05, 07 and 10 all wait for it · **Priority:** P2

The version browser has to show **who** made a version ([ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) decision 7,
issue 07), and a merge has to attribute a change to somebody. **Nothing in the epic stores a name today.** This issue
is the smallest piece of the whole package and two others reference it, which is why it is worth deciding before
either lands rather than inside one of them.

There are **two** identities here and they must not be collapsed into one:

| | Where it lives | What it is for |
|---|---|---|
| **A display identity** — an artist or user name, and an e-mail | the settings, as the default for a new project; the **project's own metadata** once it has one | attribution, the version browser's author column, a render's file name |
| **A cryptographic identity** — a key pair, with a fingerprint | the settings folder, **never in a project file** | proving a claim: a peer authenticates with it (issue 10), and an invitation cannot be forged or replayed without it |

**The first is the human-readable half of the metadata the merge needs; the second is the proof.** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md)
requires "who changed what" and a name alone cannot support that, because a name is a string anybody can set. Git is
the reference for the split and also the warning: its `user.name` and `user.email` are free strings in a config file,
and signing is a separate, optional thing most users never enable. A shared session cannot afford that weakness,
because the transport needs a key anyway for the invitation to mean anything.

**Depends on:** nothing for the settings half — that can start today. The project-metadata half rides with issue 03's
format version bump, because it adds a field to the document.

## Goal

A person says once who they are. A new project takes that as its own metadata and travels with it when it is shared,
so a version browser on the other machine can name the author. A peer proves who it is with a key that is never in
the project file.

## Scope

1. **The display identity: a name, and optionally an e-mail, in the settings.** `Settings` is a flat struct written
   as `key = value` in `settings.txt` (`crates/ui/src/settings.rs`), and it already has a page structure (General,
   Interface, Graphics, Audio, Shortcuts, Plug-ins), so this is two fields on the General page.
2. **The project carries its own metadata, and that is what travels.** A shared package must be able to name its
   author on a machine that has never met them, so the project's author is **project data**, not a setting. A new
   project takes the setting as its default and can be changed afterwards.
3. **A template, not a fixed rule, for the name of an exported file.** The default is the artist and the project;
   it is editable, because the exact convention is a matter of taste (AGENTS.md's "almost everything is
   customisable" is the principle this follows). `scripts/release_notes.py` and `scripts/discord_post.py` already
   build names from templates, so the idea has a precedent in this tree.
4. **A concrete layering problem to solve, not to discover later.** `Settings` lives in `crates/ui`, which is layer
   3, so the CLI and the session cannot reach it. **Either the identity moves down a layer or the project's metadata
   is the only thing a render or a command may read** — decide which, and do not let a command quietly grow a
   dependency on the UI crate.
5. **Decided: the project has an author, and it is the person who created it.** One field, set when the project is
   made, carried in the project file, and **inherited by a fork rather than re-declared** — so a fork of somebody
   else's project still says whose project it is, and two people opening the same project do not fight over the
   field. This is the ownership question ("whose project is this?") and it is answered by the origin, because every
   later version is a branch of it.
6. **But a version also records who made it, and that is a different question.** "Who made this version?" cannot be
   answered by the project's author: the column would be a constant, and it would be **wrong** the moment somebody
   else contributes — which is exactly the case where the column matters. So each version carries an author too,
   **defaulting to the author of the version it descends from**, and set to the current person when they differ.
   This is git's own split, and the user's framing: a repository has an owner, and every commit records who made it.
   The two never conflict — one person makes a version — while a disagreement about the project's author is a real
   disagreement worth reporting rather than resolving silently.
7. **The cryptographic identity: generated once, stored in the settings folder.** It never enters a project file and
   is never committed, and the user can see its fingerprint and replace it. This issue decides only that it exists
   and where it lives; **how it is used is issue 10's business.**
8. **Nothing here reaches the audio thread, and nothing here blocks a project from opening.** A project with no
   metadata, or with an author this machine has never heard of, opens exactly as one with them.

## Acceptance criteria

- [ ] Two fields exist on Settings › General, are written to and read from `settings.txt`, and survive a restart.
- [ ] A new project takes them as its metadata, and the metadata is written to the project file and read back
      unchanged (a round-trip test, ADR 0010's rule for every new field).
- [ ] `project.info` reports the project's author, so the CLI and MCP see the same thing the window does.
- [ ] A project shared as a package names its author on a machine whose settings say nothing about that person.
- [ ] The exported-file name template is used by whichever command names a file, its default is readable, and a
      template that produces an unusable name (empty, or containing a path separator) falls back rather than
      failing.
- [ ] **No command below the UI layer depends on `crates/ui`.** The chosen resolution of Scope 4 is stated in the
      issue and shown by the dependency graph, not asserted.
- [ ] The cryptographic identity is generated once, stored under the settings folder, reported by fingerprint, and
      **never appears in a project file** — with a test that a saved project contains no key material.
- [ ] Merging two projects that name different authors produces the decided shape (a list, or a reported conflict)
      and never silently drops one of them.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: `docs/spec/suhina-format.md` (the new metadata), `docs/decisions/0026-project-folder-and-package.md`
      if it names the fields, `docs/shortcuts.md` only if a key is added, and `ROADMAP.md`.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **Accounts, a key server, a registry, or anything the project operates.** The identity is local.
- **Signing every command in the first version.** A shared session authenticates the peer (issue 10); per-command
  attribution is a larger question and this issue does not pre-empt it.
- **Publishing or verifying an identity against a third party.** No web of trust, no certificate authority.
- **The version browser itself** (issue 07) — this issue gives it something to show, and issue 07 shows it.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): the display identity and the template in
  `settings` as the UI has it today; **the project's metadata in `model`**, then in `crates/project-file` on both
  sides and in the round-trip test. If Scope 4 is resolved by moving the identity down, that is a change to the crate
  map in `AGENTS.md` and `docs/architecture.md` and belongs in this issue's pull request.
- **Related:** ADR 0027 decision 7 (the author column the version browser needs), issue 03 (the format version bump
  the metadata rides on), issue 05 (a merge that must attribute a change), issue 07 (where the name is shown),
  issue 10 (what the key is used for), `crates/ui/src/settings.rs`.
- **Decisions that belong in an ADR or spec, not here:** which layer the display identity lives in; what an
  exported-file template may contain; and whether the key is used for anything beyond authenticating a peer in the
  first version.
- **The field shape is settled (Scope 5 and 6):** a project author that a fork inherits, plus a per-version author
  that defaults to the one it descends from. The remaining question is smaller and is about the **format**: whether
  the project-level field and the version-level field are two fields, or one, with the project's author being the
  author of its first version. Two fields are explicit and answer "whose project is this?" without walking history;
  one field cannot disagree with itself. Either satisfies the version browser; pick one when the format is written.
- **Board:** `enhancement`; **needed in the first release** — the version browser's author column and a merge's
  attribution both require it, and it is two settings fields plus a model field; Priority P2.
