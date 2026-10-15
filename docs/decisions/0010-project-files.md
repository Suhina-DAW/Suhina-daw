---
description: Suhina's own project file (.suhina; .locr from before October 2026 still read): versioned JSON of the model, written by its own crate; saving, templates and new projects
read_when: project file, save, open, .suhina, .locr, file extension, native format, project.save, templates, new project, blank project, unsaved changes, recent projects
type: decision
status: current
last_verified: 2026-10-15
covers:
  - crates/project-file
  - crates/session/src/lib.rs
  - crates/session/src/blank.rs
  - apps/suhina/src/recent.rs
---

# 0010: Suhina's own project files (.suhina)

- **Status:** accepted
- **Date:** 2026-10-12; extension changed 2026-10-13 (decision 10)

## Context

Until now a project could only be imported (a Live Set) and never saved: every edit was lost
when the window closed. Templates, new projects and "save" all need a file that Suhina writes
itself and reads back with nothing lost. `docs/architecture.md` had it planned as a native
format written and read by its own crate.

The first extension, `.locr`, stood for the product's earlier name. When the product became
Suhina (October 2026) the extension followed: files are `.suhina` from 2026-10-13, and the
`.locr` files saved before still open (decision 10).

## Decision

1. **The file is JSON**, extension `.suhina`: `{ "format": "suhina", "version": 1, "project": … }`.
   JSON over XML or a binary format because people and agents can read and diff it, the
   workspace already uses `serde_json` (keymap, themes, MCP), and the model is plain data.
   The layout is in [the spec](../spec/suhina-format.md).
2. **Its own crate, `suhina-project-file`** (`crates/project-file`; layer 1, beside `als`), maps the model to JSON and back
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
   writes the project file the project came from. It writes the running plug-ins' state as it is now:
   each host process is asked on a thread of its own and the save waits at most a second for
   the answers, so a host busy with something else (an editor opening) cannot hold the window;
   one that has not answered gives the state it last gave. It writes to a file beside the target that is then moved over it, so a
   failed save never leaves half a project. `copy: true` writes a template or backup and leaves
   the project with its own file. The session counts its changes, and `project.info` reports
   `unsaved_changes`.

   A change made only in a plug-in's editor is unsaved too (issue #362), though it is neither a
   change to the open project nor an undo step. The session compares each state it reads from a
   running plug-in (every 30 s, when the plug-in stops, and once just after it starts) with the
   state that plug-in gave when the project was last saved, or when it was first read. Two reads
   in a row that differ from it and agree with each other mark the project changed until it is
   saved; one read alone does not, so a plug-in that reports different bytes on every read (a
   counter, a random seed) never makes the project unsaved for good. Saving makes the states it
   wrote the ones compared with; a copy changes nothing. So `project.info` notices an editor
   change within two reads, about a minute, and the window reads the states twice right away
   before it asks whether to save (decision 9).
8. **New projects.** A blank project is two note tracks, two audio tracks, two returns, the main
   channel and eight scenes (`suhina_session::blank_project`); `track.add` adds more. A
   project from a template is any project file opened untitled; its audio is still
   found next to the template.
9. **The window** offers New or open (Ctrl+T): a new blank project, one from a template in the
   templates folder, or a project to open from a typed path or the recent list (kept in
   `recent.txt`); Save (Ctrl+S), Save as (Ctrl+Shift+S) and Save as template ask for a name and
   folder. The projects and templates folders are settings. There is no system file dialog: the
   dialog lists what Suhina knows, and a path can be typed or a file dropped. Closing a project,
   opening another in its place or quitting while a project has unsaved changes asks first (it
   reads the running plug-ins' states first, so a change made a moment ago in an editor counts):
   Save, Don't save or Cancel; quitting asks about each such project in turn, and Save on one
   that has no file yet asks for a name before the app closes.
10. **`.suhina`, and `.locr` still read** (2026-10-13). New projects, templates and every name the
    window or `project.save` makes end in `.suhina`, and the file names the format `"suhina"`.
    A `.locr` file, with `"format": "locr"`, is the same format: it opens, lists among the
    templates and in the browser, and is saved back to the same `.locr` path (Ctrl+S,
    `project.save` without a path, or a path that ends in `.locr`), so no file is renamed
    behind the user's back. Save as offers `.suhina`; the file saved then names the format
    `"suhina"`, which a Suhina from before the change does not read.

## Consequences

- Edits can be kept; templates and new projects work.
- `.suhina` files can be read and written by agents and scripts without Suhina.
- Readers keep accepting `.locr` and `"format": "locr"` (`suhina_project_file::is_project_file`,
  tested); a `.locr` file saved again keeps its name but names the new format inside.
- Every new field in the model needs its line in `suhina-project-file` and in the round-trip test; a
  field that changes meaning needs a new version.
- Large note-heavy projects are bigger than a binary format would make them.

## Not decided yet

- Backups and autosave ([#202](https://github.com/suhina-daw/suhina-daw/issues/202)).
- A packaged format that carries its audio, and "collect and save".
- A system file dialog for Open and Save as.
