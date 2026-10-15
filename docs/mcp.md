---
description: The MCP server for agents: tools, file roots, errors and how to run it
read_when: MCP, agents, audio output, audio device, buffer size, ASIO, project_open, command_run, file roots, rmcp, suhina-cli mcp, several projects, project id, plug-in commands, device commands, library commands, add samples, track.add, clip.addFiles, preview, preview.noteOn, preview.noteOff, note.split, mute a note, note.setMuted, note.humanize, note.fitToScale, note.invert, note.reverse, note.legato, note.stretch, note.randomizeVelocity, note.rampVelocity, MIDI tools
type: system
status: current
last_verified: 2026-10-15
covers:
  - crates/automation/src/server.rs
  - crates/automation/src/workspace.rs
  - crates/session/src/audio.rs
  - crates/automation/src/render.rs
  - crates/engine/src/cpu.rs
  - apps/suhina-cli/src/main.rs
---

# MCP server

`suhina-cli mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) on
stdin/stdout (the official `rmcp` SDK), so an agent can open, inspect, edit and render projects.
It follows the same conventions as [PhotoCraft](https://github.com/storytold/photocraft)'s MCP server.

```sh
suhina-cli mcp --read-root ./projects --write-root ./renders [--audio]
```

- **Files.** Paths are relative to a root, or absolute paths beneath it. `..` and symlinks that
  lead outside are refused. Without `--read-root` the agent cannot open files; without
  `--write-root` it cannot write any. Existing files are replaced only with `overwrite: true`.
  `project.save` writes below the write root only: with `path`, or over the project's own file
  when that file is there (`overwrite: true` needed, as for any existing file; `overwrite` is an
  MCP-only parameter, removed before the command runs).
  Commands that read files take the same paths and are checked the same way before they run:
  `clip.addFiles` (`paths`), `preview.play` (`path`), `device.add` and `track.add` (`preset`),
  `library.list` (`folder`), `library.search` (a folder `place`), and a file `item` of
  `library.info`, `library.pin`, `library.unpin` and `library.tag`; `soundmap.analyse`
  (`folder`), `soundmap.layout` (a folder `place`), and `soundmap.similar` (`item`, a folder `place`) and
  `soundmap.info` (`item`); `soundmap.setKind` (`item`); `library.findFiles` (`folder`) and `project.relinkFiles` (every
  `to` file).
- **Missing files.** `project.missingFiles` lists the audio files a project names but cannot
  find; `library.findFiles {names, folder?}` finds candidates by name (the browser's index and
  the project's folder, or a folder below the read root); `project.relinkFiles {files, to}`
  points the clips at them, one undo step ([spec](spec/missing-files.md)).
- **Several projects.** The server keeps every opened project with its own undo history, like
  tabs ([ADR 0007](decisions/0007-several-open-projects.md)). `project_open` adds one and makes
  it active; `projects.list`, `projects.switch`, `projects.next`, `projects.previous` and
  `projects.close` (through `command_run`) manage them. Tools work on the active project, or on
  the one their `project` argument names, without switching. At most 32 are open.
- **Audio output.** With `--audio`, each opened project also plays on the chosen output device
  (the default one until `audio.setOutput` chooses another) with the transport stopped, and transport commands (`scene.launch`, `clip.launch`, `track.stop`,
  `launcher.stopAll`, `launcher.setQuantization`, `launcher.backToArrangement`, `transport.play`, `transport.stop`,
  `transport.setPosition`, `transport.setMetronome`,
  `launcher.state`) are heard. Without it they return an error; launch scripts in
  `project_render` work either way. Edits and undo/redo are heard by the playing engine from its
  next audio block (0.15 s later when stretched arrangement clips must be prepared first), with
  launched clips playing on
  ([ADR 0005](decisions/0005-edits-while-playing.md)). To sweep a track's volume or pan by
  ear as one undo step, send `track.setVolume` or `track.setPan` repeatedly with
  `continue: true` after the first ([ADR 0002](decisions/0002-commands.md)). Only the active project plays: switching
  closes the previous project's output and starts the new one (its result has `audio_output`),
  so transport commands aimed at another project fail.
- **Choosing the output.** `audio.devices` lists the audio hosts (WASAPI, and ASIO in a build
  with the `asio` feature; ALSA, JACK or CoreAudio elsewhere) and each one's output devices with
  `sample_rates`, `buffer_sizes` (what the driver allows), `buffer_range` and whether WASAPI
  exclusive mode is possible, plus the output `chosen`. `audio.setOutput {host?, device?,
  sample_rate?, buffer_frames?, exclusive?}` changes the parts given (an empty text or 0 goes back
  to the default; a new host forgets the device) and reopens the active project's output there,
  the transport where it was (while playing, the play start marker moves to the playhead) and the
  plug-ins running on when the sample rate stays the same; a device that is not there plays the
  default output and the result's `notice` says so. The engine renders in blocks of
  `buffer_frames` (64 to 4096), which sets the plug-ins' deadline. `audio.info` says what plays
  now: `host`, `device`, `sample_rate`, `buffer_frames` (the device's, measured),
  `engine_block`, `exclusive`, `latency_ms` (engine to speaker, measured), `underruns` and
  `failed`. Without `--audio`, `audio.devices` returns an error.

- **Play position.** `transport.setPosition {beats}` moves the play start marker, where
  `transport.play` starts (a jump while playing). `transport.play {from: "clip", track, index}`
  moves the marker to that arrangement clip's start and plays from there. `transport.stop` returns the playhead to the
  marker; with `return: "here"` the marker moves to where it stopped; stopping while stopped
  moves the marker to beat 0. `launcher.state` reports `start_beat`. In a `project_render`
  launch script, `transport.setPosition` then `transport.play` renders from that beat
  ([arrangement editing](spec/arrangement-editing.md) › Play position).

- **Saving.** `project.save { path?, copy? }` (through `command_run`) writes the project as a
  Suhina project (`.suhina`, [spec](spec/suhina-format.md)) with its plug-ins' state as it is now;
  without a path, to the project file it came from (a `.locr` from before October 2026 stays `.locr`); `copy: true` writes a copy (a template) and the
  project keeps its own file. `project.info` reports `unsaved_changes`. `project_open` opens
  `.suhina` (and older `.locr`) files as well as Live Sets.

- **Clips.** `project.clips` lists each arrangement clip with its start, length and fades
  (lengths, shapes and curves); `clip.move`, `clip.duplicate`, `clip.delete`, `clip.copy`,
  `clip.cut`, `clip.paste`, `clip.setGain` and `clip.setFades` (fade-in and fade-out in beats,
  `inShape`/`outShape` and `inCurve`/`outCurve`) edit them through
  `command_run` ([arrangement editing](spec/arrangement-editing.md)).

- **Notes.** `note.list {track, index | scene}` lists a MIDI clip's notes (index, time and length
  in beats, key with 60 as C3, velocity 1 to 127, and `muted`); `note.add {..., notes: [{time, length, key,
  velocity?}]}` appends notes and returns their indices; `note.delete`, `note.move {beats?,
  semitones?}`, `note.resize {length? | by?}` and `note.setVelocity {velocity? | by?}` take
  `notes`, a list of those indices. Edits keep the notes' order, so the indices of notes not
  deleted stay valid; each is one undo step and fails as a whole on a bad value
  ([clip editor spec](spec/ui-clip-editor.md) › Commands). An edit to a clip whose content
  other clips also play (a duplicate) first gives that clip its own copy of the content, so the
  others do not change; the result's indices are the same.
- **Muting notes.** `note.setMuted {track, index | scene, notes, muted?}` sets the named notes'
  mute (`true` mutes, `false` unmutes); with `muted` left out each named note is flipped, which is
  what a click with the piano roll's Mute tool does. A muted note keeps its place, length, key and
  velocity and is never played (the arrangement, the launcher and an offline render all skip it);
  it is saved as `"muted": true` in the project file and survives a save and reopen. One undo step;
  it returns the notes' indices and their state.
- **Quantise.** `note.quantize {track, index | scene, notes?, grid, amount?, ends?}` moves the
  starts of the given notes, or of every note, towards the nearest line of `grid` (`"1/16"`,
  `"1/8T"`, `"1 bar"`, as `launcher.setQuantization` takes it) by `amount` percent (default 100,
  onto the line); with `ends: true` the ends move too. One undo step; a bad grid, an amount
  outside 0 to 100 or a note that does not exist fails and changes nothing.
- **MIDI tools.** The piano roll's MIDI tools column runs the same commands an agent can:
  `note.humanize {notes?, time?, velocity?, seed?}` moves each start and velocity a small random
  amount, `note.fitToScale {notes?, root?, scale?}` moves each note to the nearest key of a scale
  (a key name such as `"C"` or `"F#"`, and `major`, `minor`, `dorian`, `blues` and the rest),
  `note.invert {notes?}` mirrors the pitches, `note.reverse {notes?}` turns them round in time,
  `note.legato {notes?}` makes each note last until the next of its key, `note.stretch {notes?,
  factor}` stretches them in time (`2` is the **×2** button, `0.5` **÷2**),
  `note.randomizeVelocity {notes?, amount?, seed?}` and `note.rampVelocity {notes?, from, to}`
  change velocities. `notes` left out means every note of the clip. The two random tools take a
  `seed`, so the same call gives the same result twice and a script can repeat it; the result says
  which seed it used when none was given. Each is one undo step and fails as a whole, and none of
  them changes a note's mute.
- **Copying notes.** `note.copy {..., notes}` copies notes to the session's note clipboard (it
  changes nothing); `note.cut` copies and deletes them. `note.paste {track, index | scene, time?}`
  adds the copied notes to any MIDI clip with their spacing kept, the first at `time` (by default
  right after the copied notes), and returns their indices; `note.duplicate {..., notes}` adds a
  copy right after the notes' end. The note clipboard is apart from the clip clipboard.
- **Splitting notes.** `note.split {track, index | scene, notes, at}` splits each named note that
  spans the beat `at` into two at `at` (the first keeps the start, the second the end); a note
  that starts or ends exactly at `at` is left alone, so a second split at the same beat changes
  nothing. One undo step; it returns the indices of the notes it made, and `at` is a beat from 0
  to 120 000.
- **Follow actions.** `clip.setFollowAction {track, scene, enabled?, time?, linked?, actionA?,
  actionB?, chanceA?, chanceB?}` sets what a slot clip does after its follow time (one undo
  step); `project.launcher` lists each slot clip's `follow_action`. A `project_render` script
  picks the same random actions every time ([clip launching](spec/clip-launching.md#follow-actions)).

- **Tracks and files.** `track.add {kind?, name?, after?, device?, preset?, voice?}` adds an audio
  or MIDI track (with a device, when one is named). `clip.addFiles {paths, track?, start? |
  scene?, separate?, stack?, warp?}` places samples and MIDI files as clips, on a track or on new
  tracks (a Stems file as a group of its master, muted, and its stems, with its mastering on the
  group, [spec](spec/stems-format.md)), one after another from a beat or down the clip slots from a scene; files that cannot be
  read are listed in `skipped`. A sample whose tempo is known (metadata, name, detection) is
  warped to the project's tempo. Each is one undo step, and a playing engine hears an added
  sample from its next graph ([browser spec](spec/browser.md) › Files,
  [ADR 0011](decisions/0011-preview-and-added-audio.md)).

- **Previews.** With `--audio`, `preview.play {path, from?, track?, gain?}` previews a sample or
  a MIDI file after the main channel (a MIDI file through `track`'s instrument, or the FM synth);
  `preview.stop` stops it; `launcher.state` reports `preview`. `preview.noteOn {track, key,
  velocity?}` plays one key through the track's instrument and holds it, as the piano roll's key
  column does, until `preview.noteOff {track, key?}` lets that key go, or every held preview note
  when `key` is left out. Both are transport commands: the project, its undo history and the
  transport are untouched.

- **CPU load.** `launcher.state` reports `cpu`: the audio engine's share of wall time spent
  rendering, in percent, measured over windows of 0.2 s and smoothed so it settles within about
  a second (`0.0` before anything is measured, `null` without an audio output). The control bar shows the same number.

- **Level meters.** With `--audio`, `mixer.levels` reads the main output's and every track's
  peak and RMS level per side in dB (`null` for silence) and whether it clipped;
  `mixer.resetClip {track?, main?}` turns clip indicators off
  ([spec](spec/level-meters.md)). An agent can play a project and check its levels this way.

- **The browser's library.** The server opens the user's browser library (`browser.json` and the
  folder indexes in Suhina's settings folder) and keeps it up to date while it runs
  ([ADR 0012](decisions/0012-file-library.md)). `library.search` finds samples, MIDI files,
  presets and projects in it, the built-in effects and instruments, and installed plug-ins; `library.list`,
  `library.info`, `library.folders`, `library.refresh`, `library.recent`, `library.pinned`,
  `library.tags` read it; `library.pin`, `library.unpin`, `library.tag`, `library.setTag` and
  `library.removeTag` change pins and colour tags. `library.addFolder` and
  `library.removeFolder` are refused for agents: the user chooses the browser's folders.

- **The sample map.** The server also analyses the library's samples in the background, as the
  desktop app does ([sample map spec](spec/sample-map.md), [ADR 0013](decisions/0013-sample-map-analysis.md)).
  `soundmap.status` tells how far it is per folder; `soundmap.layout {place?, text?, tags?,
  grid?, aspect?, maxSeconds?}` lays a place's analysed samples out by how they sound and their kind
  (position 0–1, kind such as kick, snare or hat, whether it was guessed from the sound and whether it
  was set by hand, whether it matches `text` and `tags`,
  and with `grid` a cell in a grid shaped close to `aspect`, width over height); `soundmap.similar {item, count?, place?}` gives the samples that sound
  most like one; `soundmap.info {item}` its kind, where the kind comes from (`hand`, `name`,
  `guess` or `none`) and its features; `soundmap.setKind {item, kind}` sets a sample's kind by
  hand (`auto` gives it back to its name and sound); `soundmap.analyse {folder?}` analyses again.

- **Devices.** `devices.list` lists the built-in effects and instruments with every parameter (id, range,
  default, unit, options); `track.devices` lists a track's devices with their values;
  `device.add`, `device.remove`, `device.setEnabled` and `device.setParam` (through
  `command_run`) edit a track's chain, one undo step each ([ADR 0004](decisions/0004-built-in-devices.md)).
  `device.setParam` sets a plug-in device's parameter too, by the id `device.pluginParams` lists
  (a running plug-in's parameters with their ranges, default, value and the plug-in's own
  text), heard from the next block; `device.setAutomation {param, points}` automates one over the
  arrangement, `points` a flat list of beat and value (`[0, 0.2, 4, 0.8]`), `[]` removing it.
  Values are kept in the parameter's range once `device.pluginParams` has listed them.
  `track.devices` gives a plug-in's set values and any device's lanes (`automation`).

- **Automation.** `automation.addPoint`, `automation.movePoint`, `automation.deletePoint`,
  `automation.movePoints`, `automation.deletePoints` (several points of one lane in one undo
  step), `automation.clear`, `automation.setEnabled` and `automation.list` (through `command_run`) edit
  the envelopes of a track's `volume`, `pan` and `mute` (no `device`) and of any device
  parameter (`device` and its `param` id), point by point, each one undo step; values in the
  set command's unit (the volume in dB). `device.setAutomation` replaces a whole device lane, for
  built-in devices too. Setting an automated parameter by hand turns its lane off (the answer
  says `"automation": "off"`) until `automation.setEnabled` turns it on again
  ([automation spec](spec/automation.md)).
  An instrument (`fm-synth`) goes first, on a MIDI track, and plays the track's notes; effects
  follow. `device.add {preset, voice?}` adds the FM synth playing a DX7 voice from a `.syx` file. Renders and
  the playing engine run the chains.

- **Plug-ins.** `plugins.scan`, `plugins.list` and `plugins.folders` (through `command_run`)
  find and list installed plug-ins; a scan loads each new or changed plug-in file in a process
  of its own ([ADR 0006](decisions/0006-plug-in-hosting.md)). `plugins.check` lists an open
  project's plug-in devices with the installed plug-in found for each (also a VST3 version
  standing in for a VST2 one), how it was found and whether its settings carry over. `plugins.addFolder` and
  `plugins.removeFolder` are refused for agents, since a folder decides whose code a scan runs;
  the user adds folders with `suhina-cli plugins add-folder` or in the desktop app's Settings ›
  Plug-ins. `plugins.setHosting` chooses
  how a plug-in's instances share host processes (`binary`, the default, `vendor` or
  `instance`), from their next start.

- **Plug-in devices.** `device.addPlugin` adds an installed plug-in (by name, id or file, from
  `plugins.list`) to a track, an instrument first; one undo step. With an audio engine
  (`--audio`) the active project's plug-ins run: each is found in the catalog and started with
  its saved settings in the background, between commands; `device.plugin` (and `plugin` in
  `track.devices`) says where one stands (`loading`, `running`, `missing`, `crashed`, `failed`,
  or `stopped` while the project does not play) with the plug-in found, its latency, whether
  its editor is open, and how it keeps up (`blocks` given and `late_blocks` not answered in
  time, which the chain went on without; the device panel shows the share). `device.openEditor` and `device.closeEditor` open its editor in a window
  on the user's screen (above Suhina's window when the desktop app runs it, or inside its
  device panel when its settings say so). When the desktop app runs it, the editor opens in
  the background: the answer is `"editor_open": "opening"` and `device.plugin` says when it is
  open; otherwise it opens at once and the answer gives its `width` and `height`. And `device.restart`
  starts a crashed one again with its
  last settings. Renders run the installed plug-ins with their saved settings; their result's
  `plugins` lists those that are missing or did not start.

## Tools

| Tool | What it does |
|---|---|
| `project_open` | Open a Suhina project (`.suhina`, or `.locr` from before) or a `.als` Live Set below the read root as a new, active project; returns its `project` id, a summary and import warnings. |
| `project_inspect` | Summary, tracks (volume, pan, mute, devices) and arrangement clips as JSON; optional `project`. |
| `command_list` | Command ids with description, kind (`read`, `edit`, `history`, `transport`, `projects`, `plugins`, `plugin`, `library`, `audio`) and parameter schema; optional `filter`. |
| `command_run` | Run one command: `{"id": "track.setPan", "params": {"track": "Bass", "pan": -1}}`; optional `project`. |
| `command_batch` | Run `steps` in order; each editing command is its own undo step; `stop_on_error` (default true); optional `project` for every step. |
| `project_render` | Render the active project (or `project`): the arrangement, or `scene` (1-based), to a WAV below the write root. Audio files are loaded from the opened set's folder; missing ones are listed in `warnings`. Its plug-ins start with the state the project's running plug-ins have now (what their editors changed included), as `project.save` writes it. With `launches` (and `seconds`), render a launch script instead: transport commands run at times in seconds from a stopped transport, e.g. `[{"at": 0, "id": "scene.launch", "params": {"scene": 1}}, {"at": 4, "id": "track.stop", "params": {"track": "Bass"}}]`; each step's result is returned. |

Commands are defined once in `suhina-session` (ADR 0002); the MCP server, the CLI and the
desktop app all run the same ones. [`commands.md`](commands.md) lists every command with its
parameters; it is generated from the registry, so a new command needs no edit there.

## Errors

- Unknown tool arguments return JSON-RPC error `-32602` naming the key, before anything runs.
- A command that fails returns a tool result with `isError: true` and leaves the project
  unchanged. Unknown command parameters are errors too.
- Every tool has a title and read-only/destructive hints.

## Using it from Claude Code

```sh
claude mcp add suhina -- /path/to/suhina-cli mcp --read-root /path/to/projects --write-root /path/to/renders
```
