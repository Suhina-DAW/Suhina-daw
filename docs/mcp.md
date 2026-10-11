---
description: The MCP server for agents: tools, file roots, errors and how to run it
read_when: MCP, agents, project_open, command_run, file roots, rmcp, loopwerk-cli mcp
type: system
status: current
last_verified: 2026-10-11
covers:
  - crates/automation/src/server.rs
  - crates/automation/src/workspace.rs
---

# MCP server

`loopwerk-cli mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) on
stdin/stdout (the official `rmcp` SDK), so an agent can open, inspect, edit and render projects.
It follows the same conventions as PhotoCraft's MCP server.

```sh
loopwerk-cli mcp --read-root ./projects --write-root ./renders [--audio]
```

- **Files.** Paths are relative to a root, or absolute paths beneath it. `..` and symlinks that
  lead outside are refused. Without `--read-root` the agent cannot open files; without
  `--write-root` it cannot write any. Existing files are replaced only with `overwrite: true`.
- **One session.** The server holds one open project with its undo history. `project_open`
  replaces it.
- **Audio output.** With `--audio`, each opened project also plays on the default output device
  with the transport stopped, and transport commands (`scene.launch`, `clip.launch`, `track.stop`,
  `launcher.stopAll`, `launcher.setQuantization`, `transport.play`, `transport.stop`,
  `launcher.state`) are heard. Without it they return an error; launch scripts in
  `project_render` work either way. Edits and undo/redo are heard by the playing engine about
  0.15 s after the command returns, with launched clips playing on
  ([ADR 0005](decisions/0005-edits-while-playing.md)).

## Tools

| Tool | What it does |
|---|---|
| `project_open` | Open a `.als` Live Set below the read root; returns a summary and import warnings. |
| `project_inspect` | Summary, tracks (volume, pan, mute, devices) and arrangement clips as JSON. |
| `command_list` | Command ids with description, kind (`read`, `edit`, `history`, `transport`) and parameter schema; optional `filter`. |
| `command_run` | Run one command: `{"id": "track.setPan", "params": {"track": "Bass", "pan": -1}}`. |
| `command_batch` | Run `steps` in order; each editing command is its own undo step; `stop_on_error` (default true). |
| `project_render` | Render the arrangement, or `scene` (1-based), to a WAV below the write root. Audio files are loaded from the opened set's folder; missing ones are listed in `warnings`. With `launches` (and `seconds`), render a launch script instead: transport commands run at times in seconds from a stopped transport, e.g. `[{"at": 0, "id": "scene.launch", "params": {"scene": 1}}, {"at": 4, "id": "track.stop", "params": {"track": "Bass"}}]`; each step's result is returned. |

Commands are defined once in `loopwerk-session` (ADR 0002); the MCP server, the CLI and later
the UI all run the same ones.

## Errors

- Unknown tool arguments return JSON-RPC error `-32602` naming the key, before anything runs.
- A command that fails returns a tool result with `isError: true` and leaves the project
  unchanged. Unknown command parameters are errors too.
- Every tool has a title and read-only/destructive hints.

## Using it from Claude Code

```sh
claude mcp add loopwerk -- /path/to/loopwerk-cli mcp --read-root /path/to/projects --write-root /path/to/renders
```
