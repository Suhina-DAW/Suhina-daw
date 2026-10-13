---
description: Suhina's own project file (.locr): versioned JSON of the model, written by its own crate; saving, templates and new projects
read_when: project file, save, open, .locr, native format, project.save, templates, new project, blank project, unsaved changes, recent projects
type: decision
status: current
last_verified: 2026-10-12
covers:
  - crates/locr
  - crates/session/src/blank.rs
  - apps/suhina/src/recent.rs
---

# 0010: Suhina's own project files (.locr)

- **Status:** accepted
- **Date:** 2026-10-12

## Context

Until now a project could only be imported (a Live Set) and never saved: every edit was lost
when the window closed. Templates, new projects and "save" all need a file that Suhina writes
itself and reads back with nothing lost. `docs/architecture.md` had it planned as a native
format written and read by its own crate.

The product is to be renamed Loopcraft; the file extension is chosen for that name now, so
projects saved today keep working after the rename.

## Decision

1. **The file is JSON**, extension `.locr`: `{ "format": "locr", "version": 1, "project": … }`.
   JSON over XML or a binary format because people and agents can read and diff it, the
   workspace already uses `serde_json` (keymap, themes, MCP), and the model is plain data.
   The layout is in [the spec](../spec/locr-format.md).
2. **Its own crate, `suhina-locr`** (layer 1, beside `als`), maps the model to JSON and back
   by hand, as the importers do; the model keeps no serialisation code or dependency
   ([ADR 0001](0001-daw-neutral-model.md)). Everything in the model is written, extensions
   included, so a project saved and opened again is equal to the one saved (round-trip tests).
3. **Versioned.** `version` is 1. A reader refuses a newer version instead of guessing; a later
   version that changes the layout keeps reading the older ones.
4. **Hostile input is an error, never a crash** (AGENTS.md rule 2): every value is checked
   (types, ranges, ids that fit), the file is at most 512 MiB, and the project must validate as
   an imported one must.
5. **Binary data is base64**: plug-in state and format extensions.
6. **Audio is referred to, not copied.** Each region keeps its path relative to the project file
   and an absolute path for when that does not resolve. Saving somewhere else rewrites both from
   where the file is found now. A self-contained package (a zip with the audio) is a separate
   format for sharing, not decided here.
7. **Saving is a command**, `project.save { path?, copy? }` (AGENTS.md rule 6). Without a path it
   writes the `.locr` the project came from. It writes the running plug-ins' state as it is now:
   each host process is asked on a thread of its own and the save waits at most a second for
   the answers, so a host busy with something else (an editor opening) cannot hold the window;
   one that has not answered gives the state it last gave. It writes to a file beside the target that is then moved over it, so a
   failed save never leaves half a project. `copy: true` writes a template or backup and leaves
   the project with its own file. The session counts its changes, and `project.info` reports
   `unsaved_changes`.
8. **New projects.** A blank project is two note tracks, two audio tracks, two returns, the main
   channel and eight scenes (`suhina_session::blank_project`); `track.add` adds more. A
   project from a template is any project file opened untitled; its audio is still
   found next to the template.
9. **The window** offers New or open (Ctrl+T): a new blank project, one from a template in the
   templates folder, or a project to open from a typed path or the recent list (kept in
   `recent.txt`); Save (Ctrl+S), Save as (Ctrl+Shift+S) and Save as template ask for a name and
   folder. The projects and templates folders are settings. There is no system file dialog: the
   dialog lists what Suhina knows, and a path can be typed or a file dropped.

## Consequences

- Edits can be kept; templates and new projects work.
- `.locr` files can be read and written by agents and scripts without Suhina.
- Every new field in the model needs its line in `suhina-locr` and in the round-trip test; a
  field that changes meaning needs a new version.
- Large note-heavy projects are bigger than a binary format would make them.

## Not decided yet

- Asking before closing a project or quitting with unsaved changes; autosave.
- A packaged format that carries its audio, and "collect and save".
- A system file dialog for Open and Save as.
