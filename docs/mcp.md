---
description: The MCP server for agents: tools, file roots, errors and how to run it
read_when: MCP, agents, project_open, command_run, file roots, rmcp, loopwerk-cli mcp, several projects, project id, plug-in commands, device commands
type: system
status: current
last_verified: 2026-10-12
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
- **Several projects.** The server keeps every opened project with its own undo history, like
  tabs ([ADR 0007](decisions/0007-several-open-projects.md)). `project_open` adds one and makes
  it active; `projects.list`, `projects.switch`, `projects.next`, `projects.previous` and
  `projects.close` (through `command_run`) manage them. Tools work on the active project, or on
  the one their `project` argument names, without switching. At most 32 are open.
- **Audio output.** With `--audio`, each opened project also plays on the default output device
  with the transport stopped, and transport commands (`scene.launch`, `clip.launch`, `track.stop`,
  `launcher.stopAll`, `launcher.setQuantization`, `transport.play`, `transport.stop`,
  `transport.setPosition`, `transport.setMetronome`,
  `launcher.state`) are heard. Without it they return an error; launch scripts in
  `project_render` work either way. Edits and undo/redo are heard by the playing engine from its
  next audio block (0.15 s later when stretched arrangement clips must be prepared first), with
  launched clips playing on
  ([ADR 0005](decisions/0005-edits-while-playing.md)). Only the active project plays: switching
  closes the previous project's output and starts the new one (its result has `audio_output`),
  so transport commands aimed at another project fail.

- **Play position.** `transport.setPosition {beats}` moves the play start marker, where
  `transport.play` starts (a jump while playing). `transport.stop` returns the playhead to the
  marker; with `return: "here"` the marker moves to where it stopped; stopping while stopped
  moves the marker to beat 0. `launcher.state` reports `start_beat`. In a `project_render`
  launch script, `transport.setPosition` then `transport.play` renders from that beat
  ([arrangement editing](spec/arrangement-editing.md) › Play position).

- **Saving.** `project.save { path?, copy? }` (through `command_run`) writes the project as a
  Loopwerk project (`.locr`, [spec](spec/locr-format.md)) with its plug-ins' state as it is now;
  without a path, to the `.locr` it came from; `copy: true` writes a copy (a template) and the
  project keeps its own file. `project.info` reports `unsaved_changes`. `project_open` opens
  `.locr` files as well as Live Sets.

- **Clips.** `project.clips` lists each arrangement clip with its start, length and fades;
  `clip.move`, `clip.duplicate`, `clip.delete`, `clip.copy`, `clip.cut`, `clip.paste`,
  `clip.setGain` and `clip.setFades` (fade-in and fade-out in beats) edit them through
  `command_run` ([arrangement editing](spec/arrangement-editing.md)).

- **Devices.** `devices.list` lists the built-in devices with every parameter (id, range,
  default, unit, options); `track.devices` lists a track's devices with their values;
  `device.add`, `device.remove`, `device.setEnabled` and `device.setParam` (through
  `command_run`) edit a track's chain, one undo step each ([ADR 0004](decisions/0004-built-in-devices.md)).
  An instrument (`fm-synth`) goes first and plays the track's notes; effects follow. Renders and
  the playing engine run the chains.

- **Plug-ins.** `plugins.scan`, `plugins.list` and `plugins.folders` (through `command_run`)
  find and list installed plug-ins; a scan loads each new or changed plug-in file in a process
  of its own ([ADR 0006](decisions/0006-plug-in-hosting.md)). `plugins.check` lists an open
  project's plug-in devices with the installed plug-in found for each (also a VST3 version
  standing in for a VST2 one), how it was found and whether its settings carry over. `plugins.addFolder` and
  `plugins.removeFolder` are refused for agents, since a folder decides whose code a scan runs;
  the user adds folders with `loopwerk-cli plugins add-folder`. `plugins.setHosting` chooses
  how a plug-in's instances share host processes (`binary`, the default, `vendor` or
  `instance`), from their next start.

- **Plug-in devices.** `device.addPlugin` adds an installed plug-in (by name, id or file, from
  `plugins.list`) to a track, an instrument first; one undo step. With an audio engine
  (`--audio`) the active project's plug-ins run: each is found in the catalog and started with
  its saved settings in the background, between commands; `device.plugin` (and `plugin` in
  `track.devices`) says where one stands (`loading`, `running`, `missing`, `crashed`, `failed`,
  or `stopped` while the project does not play) with the plug-in found, its latency and whether
  its editor is open. `device.openEditor` and `device.closeEditor` open its editor in a window
  on the user's screen (above Loopwerk's window when the desktop app runs it, where it opens
  in the background: the answer is `"editor_open": "opening"` and `device.plugin` says when it
  is open), and `device.restart` starts a crashed one again with its
  last settings. Renders run the installed plug-ins with their saved settings; their result's
  `plugins` lists those that are missing or did not start.

## Tools

| Tool | What it does |
|---|---|
| `project_open` | Open a Loopwerk project (`.locr`) or a `.als` Live Set below the read root as a new, active project; returns its `project` id, a summary and import warnings. |
| `project_inspect` | Summary, tracks (volume, pan, mute, devices) and arrangement clips as JSON; optional `project`. |
| `command_list` | Command ids with description, kind (`read`, `edit`, `history`, `transport`, `projects`, `plugins`) and parameter schema; optional `filter`. |
| `command_run` | Run one command: `{"id": "track.setPan", "params": {"track": "Bass", "pan": -1}}`; optional `project`. |
| `command_batch` | Run `steps` in order; each editing command is its own undo step; `stop_on_error` (default true); optional `project` for every step. |
| `project_render` | Render the active project (or `project`): the arrangement, or `scene` (1-based), to a WAV below the write root. Audio files are loaded from the opened set's folder; missing ones are listed in `warnings`. With `launches` (and `seconds`), render a launch script instead: transport commands run at times in seconds from a stopped transport, e.g. `[{"at": 0, "id": "scene.launch", "params": {"scene": 1}}, {"at": 4, "id": "track.stop", "params": {"track": "Bass"}}]`; each step's result is returned. |

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
