# Keep a kind of sound set by hand in its own store, away from the versioned feature cache

**Type:** enhancement · **Area:** `browser`, `formats` · **Phase:** M8 or later · **Priority:** P2

The sample map decides a sample's kind — kick, snare, clap, hats, toms, cymbals, percussion, bass, tonal, vocal, effect — from words in its name, and otherwise from what its nearest neighbours are (ADR 0013 §7). A user who corrects one is making a **decision**, and a decision cannot be recomputed. The obvious place to put it is the feature cache, and that would be a bug: **the cache is versioned**, and a cache entry written by another feature version is ignored and the folder analysed again (ADR 0013 §8). The first features-v2 bump would silently throw away every correction. This issue gives that data its own store. It closes [#325](https://github.com/suhina-daw/suhina-daw/issues/325).

**Depends on:** nothing to start — but the store's precedence rule belongs in an ADR or in the [sample map spec](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/sample-map.md) before the code (AGENTS.md rule 1).

## Goal

A kind the user set is remembered for those exact bytes, at any path, in any project, and it survives every feature-version bump the analysis ever has.

## Why a content hash and not a path

The epic's measurement is the argument: `.asd`-style sidecars written next to samples were **one per audio file, 48 % of the files in the sample folder and 34 % of the whole project, for 2 % of its bytes** — and they do not even follow the file, because copying just the audio leaves them behind. The user's goal is *"this sample is always recognised as this kind of thing"*, and the only key that delivers it is the bytes:

- **content-hash-keyed** — the same audio at a new path, or copied into another project, is recognised
- **in the settings folder**, not next to the sample — sample folders can be network drives, read-only or slow, and ADR 0012 already knows this
- **one store, not one file per sample** — a library of 200 000 samples must not gain 200 000 files

An explicit **export beside the files** (the same shape as collecting a project, issue 03) is how a sample pack travels with its kinds, and it is a separate action, never the default. If it is built, it writes **one file per folder**, not per sample.

## Scope

1. **A store of its own**, unversioned and keyed by content hash, holding a kind per hash. Its own file, its own format, its own version header **that does not gate the data** — a reader from a later Suhina reads what an earlier one wrote, and neither discards the other's kinds.
2. **Precedence, written down:** a project's own override, then the store, then the guess (name, then nearest neighbours). The first of these may not exist yet; if not, say so in the spec rather than leaving the order implicit.
3. **A command to set a kind** (`library.setKind` or the sample map's own family, following ADR 0013's `soundmap.*`), one undo step where it touches the project, and the same command from UI, CLI and MCP (AGENTS.md rule 6).
4. **The sample map's own read** reports where a kind came from — set by hand, from the name, or guessed — because a user who cannot tell a correction from a guess will not trust either.
5. **Surviving a feature bump**, proved by a test rather than asserted.
6. Tests (AGENTS.md rule 7).

## Acceptance criteria

- [ ] A kind set by hand survives an analysis with a **different feature version** — a test that writes a kind, bumps the feature version, re-analyses, and finds the kind intact. **This test fails on the current design**, which is the point of the issue.
- [ ] A kind set for a file is found again for the same bytes at a different path, in a different folder and inside a different project.
- [ ] The store is content-hash-keyed: two files with identical bytes share one entry, and a file whose bytes changed is a different entry.
- [ ] Setting a kind writes nothing into the sample's own folder, and works when that folder is read-only.
- [ ] A library of many samples gains **one** store file, not one file per sample.
- [ ] A store file that does not parse, is truncated, or holds an absurd number of entries is ignored or refused by name, and the sample map still opens (AGENTS.md rule 2).
- [ ] The commands work from UI, CLI and MCP, and `docs/commands.md` regenerates on `main`.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: `docs/spec/sample-map.md`, `docs/decisions/0013-sample-map-analysis.md` (its cache section is where the separation is recorded), `ROADMAP.md`.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **A trained classifier.** ADR 0013 §7 says "no trained model in this version"; this issue stores a human's correction and does not learn from it.
- **Features v2** — chroma, tempo-aware windows, classifying before the layout. [#325](https://github.com/suhina-daw/suhina-daw/issues/325) also covers that; this issue is only the storage of a hand-set kind, and should be splittable from the recognition work.
- The project's own override store, if it is not already expressible. If it is needed, it is model data and belongs in its own issue.
- Any sidecar written next to a sample by default.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): `crates/soundmap` owns the features, the layouts and the cache (ADR 0013 §1); the store is a sibling of that cache. `session` runs the `soundmap.*` commands on the `SoundMap` that `Projects` holds; `ui` dispatches them from the sample map panel. Nothing here touches the audio thread, and analysis already runs on a pool of half the cores.
- **A note on the cache's location, for the ADR:** ADR 0013 §8 puts the feature cache in `<settings>/library/features-<hash>.bin`, one per library folder. The store needs a location of its own and a lifetime that is not tied to a folder, because a correction outlives the folder it was made in; whether it is one file beside the caches or one file per folder is the ADR's call, and either way it must not be discarded with them.
- **Related:** [#325](https://github.com/suhina-daw/suhina-daw/issues/325) (what this closes), ADR 0013 (the kinds, the cache, its versioning), ADR 0012 (sample folders on network drives), [docs/spec/sample-map.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/sample-map.md), [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › "Derived analysis stays out of the project", issue 03 (collecting, the same explicit-action shape).
- **Decisions that belong in an ADR or the spec, not here:** the store's location and format; whether a kind is global by content or project-scoped with a global default — **this one must be settled before the store is designed, because it decides the lookup order**; whether a same-name-different-bytes file is treated as the same sample; and whether the store is ever exported beside the files.
- **Board:** `enhancement`; Phase M8 or later; Priority P2. Small enough to be a `good first issue` only after the precedence decision is written.
