---
description: Why every action is a command with an id, typed params and snapshot undo, shared by CLI, MCP and UI
read_when: adding a command, undo, redo, command registry, params, CommandSpec
type: decision
status: current
last_verified: 2026-10-11
covers:
  - crates/session/src/commands.rs
  - crates/session/src/lib.rs
---

# 0002: Every action is a command

- **Status:** accepted
- **Date:** 2026-10-11

## Context

Loopwerk will be driven from several places: the CLI, an MCP server for agents, later a UI and
MIDI controllers. If each of them changed the project in its own way, they would drift apart,
undo would only work in some of them, and an agent could not do everything a person can.

PhotoCraft (MIT/Apache-2.0) solves the same problem with a command registry: every menu item,
tool and dialog is a command with an id, a parameter description and a `run` function, and the
UI, CLI, control channel and MCP server all dispatch commands by id. We take the idea, not code.

## Decision

A new crate, `loopwerk-session`, holds a **session**: the open project, where it came from, and
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

Launching clips and starting or stopping playback are commands too, of a fourth kind,
**transport** (next to read, edit and history). They act on the audio engine attached to the
session (a `Player`: the renderer's lock-free command queue and its state monitor), never change
the project and record no undo step. Without an attached engine they return an error that says
how to start one. `launcher.state` reads the engine's state the same way. See
`docs/spec/clip-launching.md`.

## Consequences

- New behaviour starts as a command with tests, then gets a UI. The UI never edits the model
  directly.
- Command ids are part of the public interface for agents; renaming one is a breaking change.
- Saving to `.als` is not possible yet (there is no writer), so sessions opened from Live Sets
  can be changed, rendered and played, but not saved. A native project format comes later.
