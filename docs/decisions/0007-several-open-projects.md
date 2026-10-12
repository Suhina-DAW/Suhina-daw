---
description: Several projects open at once like tabs, one active; where commands go, which project plays, and what is left for later
read_when: tabs, several projects, open projects, switch project, Ctrl+Tab, projects.list, projects.switch, Projects, ProjectId
type: decision
status: current
last_verified: 2026-10-12
covers:
  - crates/session/src/projects.rs
---

# 0007: Several open projects

- **Status:** accepted
- **Date:** 2026-10-11
- **Task:** [#36](https://github.com/juhamikael/Loopwerk/issues/36)

## Context

Users asked to keep several projects open and switch between them with Ctrl+Tab, as some DAWs
show projects in tabs. The value is in moving between songs without closing them: comparing
mixes, and later copying clips, tracks and devices from one song to another.

Until now a `Session` was the one open project, and the MCP server replaced it on every
`project_open`. Nothing in the model or engine assumes a single project, so the change is in
the layer that owns sessions.

## Decision

1. **`Projects` in `loopwerk-session`** holds the open sessions in tab order and which one is
   active. Each keeps its own project, source and undo history. A `ProjectId` names an open
   project and is never reused while the `Projects` lives, so a stale id cannot reach a
   project opened later. At most 32 are open at once.
2. **Commands go to the active project** unless the caller names another
   (`Projects::execute_in`, the MCP tools' `project` argument). Naming a project does not switch
   to it.
3. **Managing projects is a command too** (AGENTS.md rule 6), of the new kind `projects`:
   `projects.list`, `projects.switch`, `projects.next`, `projects.previous` (wrapping around,
   for Ctrl+Tab and Ctrl+Shift+Tab) and `projects.close`. Closing the active project activates
   the tab to its right, or to its left when it was the last. Opening stays outside the
   registry for now, since it needs file access that each front end grants differently (the
   MCP server's read root).
4. **Only the active project plays.** Switching closes the previous project's `Player`; the
   front end starts one for the new active project. Two projects never mix into one output.

## Consequences

- A tab shows its project's file name; a project saved under a new name (`project.save` with a
  path, not a copy) is shown by it from then on.

- The MCP server holds `Projects`: `project_open` adds a project and makes it active, and with
  `--audio` a switch starts the new project playing.
- A switch with audio reloads the new project's audio files and opens a new output stream, so
  it is not instant for large sets. Keeping one output open and swapping the engine graph
  ([ADR 0005](0005-edits-while-playing.md)) makes it so; that is a follow-up.
- Background projects stay in memory with their undo history; decoded audio is held only by
  the active project's player over MCP. The desktop app shows the projects as tabs and keeps
  each open project's decoded audio, so a switch there opens only a new output stream.
- Only the active project's plug-ins run: `Projects::poll_plugins` starts them while it plays,
  and a project that stops playing (another tab made active) stops them, each keeping its last
  state for when the project plays again ([ADR 0006](0006-plug-in-hosting.md)).
- `Projects` is where Loopwerk-wide state lives too: it also holds the plug-in library that the
  `plugins.*` commands run on ([ADR 0006](0006-plug-in-hosting.md)), shared by every open
  project.

## Not decided yet

- **Copying between projects:** commands that copy a clip, track or device chain into another
  open project, with the files they refer to.
- **Background playback:** whether a background project may keep playing (for example a
  reference track), and how its output is routed.
- **Unsaved changes on close:** needs the native project format; today closing drops edits.
