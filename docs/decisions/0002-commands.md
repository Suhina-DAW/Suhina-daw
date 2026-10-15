---
description: Why every action is a command with an id, typed params and snapshot undo, shared by CLI, MCP and UI
read_when: adding a command, undo, redo, command registry, params, CommandSpec
type: decision
status: current
last_verified: 2026-10-15
covers:
  - crates/session/src/commands.rs
  - crates/session/src/lib.rs
  - crates/session/src/projects.rs
  - crates/session/src/plugins.rs
  - crates/session/src/library.rs
  - crates/session/src/audio.rs
  - crates/session/src/devices.rs
---

# 0002: Every action is a command

- **Status:** accepted
- **Date:** 2026-10-11

## Context

Suhina will be driven from several places: the CLI, an MCP server for agents, later a UI and
MIDI controllers. If each of them changed the project in its own way, they would drift apart,
undo would only work in some of them, and an agent could not do everything a person can.

[PhotoCraft](https://github.com/storytold/photocraft) (MIT/Apache-2.0) solves the same problem with a command registry: every menu item,
tool and dialog is a command with an id, a parameter description and a `run` function, and the
UI, CLI, control channel and MCP server all dispatch commands by id. We take the idea, not code.

## Decision

A new crate, `suhina-session`, holds a **session**: the open project, where it came from, and
its undo history. All changes to a session go through **commands**.

- A command has a stable **id** in `area.verb` form (`track.setPan`, `edit.undo`), a short label,
  a parameter description, whether it changes the project, and a `run` function.
- **Parameters and results are JSON.** That is what MCP and the CLI speak, and it keeps the
  registry independent of any one front end. Tracks are named by id or by name.
- A command that changes the project records **exactly one undo step**. If it fails, or leaves a
  project that does not validate, the session is restored to how it was and the command returns
  an error; it never panics, whatever the parameters.
- **Undo is a snapshot of the project** before the change. Projects are small (no audio data), so
  this is simple and always correct. If memory becomes a problem we switch to diffs without
  changing the command interface.
- Reading commands (`project.info`, `project.tracks`) are commands too, so agents discover the
  project the same way they change it.
- `commands.list` returns every command with its parameters; front ends build their help and the
  MCP tool list from it.

Layers: `model` (L0) ← `als`, `engine` (L1) ← `session` (L2) ← CLI, MCP, UI.

## Amendment: transport commands (2026-10-11)

Launching clips, starting or stopping playback and moving where it starts
(`transport.setPosition`) are commands too, of a fourth kind,
**transport** (next to read, edit and history). They act on the audio engine attached to the
session (a `Player`: the renderer's lock-free command queue and its state monitor), never change
the project and record no undo step. Without an attached engine they return an error that says
how to start one. `launcher.state` reads the engine's state the same way. See
`docs/spec/clip-launching.md`.

## Amendment: project commands (2026-10-11)

With several projects open ([ADR 0007](0007-several-open-projects.md)), a fifth kind,
**projects**, lists, switches and closes the open projects (`projects.*`). These run on
`Projects`, not on one session, never change a project and record no undo step. `CommandSpec`
is generic over what a command runs on, so both kinds share one shape, one parameter schema and
one `commands.list`.

## Amendment: plug-in commands (2026-10-12)

A sixth kind, **plugins**, scans for installed plug-ins and chooses the folders scans look in
(`plugins.*`, [ADR 0006](0006-plug-in-hosting.md)). The plug-in library belongs to Suhina,
not to a project, so these also run on `Projects`, never change a project and record no undo
step. Agents over MCP may scan and list but not add or remove folders: a folder decides whose
code a scan runs.

## Amendment: running plug-ins (2026-10-12)

A seventh kind, **plugin**, acts on one of the project's running plug-ins:
`device.openEditor`, `device.closeEditor` and `device.restart` ([ADR 0006](0006-plug-in-hosting.md)).
They run on the session, never change the project and record no undo step. Adding a plug-in
(`device.addPlugin`) is an edit like `device.add`; `device.plugin` reads where a plug-in device
stands. `plugins.setHosting` (kind plugins) chooses how a plug-in shares host processes.

An eighth kind, **library**, runs on `Projects` like the plugins kind: the browser's folders,
search, pins, recent items and colour tags (`library.*`, [ADR 0012](0012-file-library.md)). It
never changes a project. A successful edit that adds something from the browser (`clip.addFiles`,
`track.add`, `device.add`, `device.addPlugin`) is recorded among the library's recent items by
`Projects`, so the CLI, MCP and the window all keep them. Parameters may also be a list of
texts (`ParamType::Texts`, `clip.addFiles`' `paths`), shown to agents as an array of strings.
The sample map's commands (`soundmap.*`, [ADR 0013](0013-sample-map-analysis.md)) are of the
library kind too: they read the library's samples and the analysis `Projects` keeps beside it.

A ninth kind, **audio**, also runs on `Projects`: `audio.devices` lists the system's audio hosts
and output devices, `audio.setOutput` chooses one (host, device, sample rate, buffer size,
WASAPI exclusive mode) and reopens the active project's output there, and `audio.info` says what
plays now. The session only describes the choice; the output layer lists devices and opens
streams through the `AudioSystem` trait it implements and the front end hands to `Projects`
(the desktop app always, the MCP server with `--audio`). It never changes a project.

## Amendment: device commands (2026-10-12)

Devices on tracks are edited with ordinary **edit** commands, one undo step each:
`device.add`, `device.addSd`, `device.remove`, `device.setEnabled`, `device.setParam`; `devices.list` and
`track.devices` read. A device is addressed by its track and its index in the track's chain
(`device`, from 0, as `track.devices` lists them), and a parameter by its stable id
([ADR 0004](0004-built-in-devices.md)). Agents may run all of them.

`device.setParam` accepts `continuous: true` during a slider drag to share one undo snapshot
across updates of the same parameter. A final call without that flag ends the gesture. Every
effective update still marks the project changed, including updates after saving mid-gesture.

## Amendment: one drag, one undo step (2026-10-15)

A value dragged in the window (a track's pan or volume) is heard while it moves, so the window
runs the command on every frame of the drag, not only on release
([#313](https://github.com/suhina-daw/suhina-daw/issues/313)). One drag still records one undo
step: `track.setVolume` and `track.setPan` take an optional `continue`. With `continue: true`
the change joins the undo step that the previous call of the same command on the same track
made, as long as nothing else happened in between; any other edit, a call without `continue`
(even one that changes nothing), undo and redo end that step. The window sends the first frame
of a drag without `continue` and the rest, and the release, with it; a double-click reset is a
step of its own. Agents can do the same over MCP.

A dragged value reaches the playing engine as any edit does, a new graph at the next block
([ADR 0005](0005-edits-while-playing.md)). A drag can send graphs faster than the engine takes
them (the queue holds 16, and graphs with stretched audio wait 0.15 s before they take over); a
continued call whose graph does not fit is therefore not an error. The project keeps the
value and `Session::catch_up_player`, which the window calls every frame through
`Projects::poll_plugins`, sends the project again until the engine has it.

## Consequences

- New behaviour starts as a command with tests, then gets a UI. The UI never edits the model
  directly.
- Command ids are part of the public interface for agents; renaming one is a breaking change.
- The registry is the only list of commands: [`docs/commands.md`](../commands.md) is generated
  from it (`suhina-cli commands --markdown`) on `main` after every merge.
- Suhina does not write `.als` (there is no writer). A session opened from a Live Set is saved
  as a Suhina project (`.suhina`) with `project.save` and a path ([ADR 0010](0010-project-files.md)).
