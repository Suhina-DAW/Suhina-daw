# Roadmap

Loopwerk is pre-alpha. Milestones are in order; each one builds on the previous.

| | Milestone | Status |
|---|---|---|
| M0 | **Foundation:** workspace, GPL-3.0-or-later, clean-room rules, DAW-neutral model | done |
| M1 | **First sound:** offline renderer and the `.als` reader | done |
| M2 | **Real-time playback:** audio output, `loopwerk-cli play` | done (verified on Windows) |
| M3 | **Commands and agents:** command registry with undo; CLI and MCP server on the same commands | in progress |
| M4 | **Clip launching:** launch quantisation, scenes, stop buttons, follow actions | in progress |
| M5 | **Audio playback:** imported audio clips with gain, transpose, warping and time-stretching | in progress |
| M6 | **Plug-ins:** CLAP hosting, sandboxed out of process (AGENTS.md rule 5); VST3 after | |
| M7 | **More formats:** DAWproject import and export | |
| M8 | **UI:** Session and Arrangement views on top of the commands; familiar workflow, Loopwerk's own look; installers (AppImage, deb, Flatpak, MSI) and signed macOS and Windows builds | |

Later, not ordered: FL Studio projects (`.flp`), built-in instruments and effects, automation,
sends and returns in the mix, MIDI controllers and Push (from Ableton's published Push interface
docs), Max for Live devices as placeholders, ARA, bridging Windows plug-ins.

Linux is the primary platform; Windows and macOS are supported.

## What works now

### M0 Foundation
- Rust workspace, GPL-3.0-or-later, CI (fmt, clippy `-D warnings`, tests) on Linux and Windows.
- Clean-room rules for people and agents ([`CONTRIBUTING.md`](CONTRIBUTING.md)) and the agent
  guide ([`AGENTS.md`](AGENTS.md)): no panics, no `unsafe`, real-time safety, sandboxed plug-ins.
- DAW-neutral project model: tracks and mixer channels apart, a pool of clip contents placed by
  clips, so per-track clips and shared patterns both fit
  ([ADR 0001](docs/decisions/0001-daw-neutral-model.md)).

### M1 Reading Live Sets
From Live Sets we authored ourselves in Live 12.4 ([format spec](docs/spec/als-format.md),
[corpus guide](docs/spec/als-corpus.md)):
- tempo and time signature (read from their automation default, where Live keeps them)
- MIDI, audio, group and return tracks; volume, pan, track on/off, sends, routing into groups
- scenes, session clip slots and arrangement clips
- MIDI clips with notes (velocity, release velocity, deactivated notes), start and end markers,
  loop, deactivated clips
- audio clips: file reference, warp on/off, warp markers, warp mode, clip gain, transpose
- device chains: VST2 and VST3 plug-ins with their id and state kept, Live's own devices as
  placeholders
- anything not understood yet is skipped with a warning; the project still opens

### M2 Playing
- `loopwerk-cli play` plays a set, or one launcher scene, on the default audio device; `render`
  writes a 32-bit float WAV. Both run the same engine code.
- MIDI notes play as sine tones until instruments arrive. Group and master volume and mute apply.

### M3 Commands and agents (in progress)
- Every edit is a command with an id, typed parameters and one undo step; failed or invalid edits
  leave the project unchanged ([ADR 0002](docs/decisions/0002-commands.md)).
- Commands so far: `commands.list`, `project.info`, `project.tracks`, `project.clips`,
  `transport.setTempo`, `track.setVolume`, `track.setPan`, `track.setMute`, `clip.move`,
  `edit.undo`, `edit.redo`.
- `loopwerk-cli mcp` serves them to agents over the Model Context Protocol, with file access
  only below granted roots ([`docs/mcp.md`](docs/mcp.md)).
- Still to do: more commands (notes, clips, sends, tracks), a native project format so edits can
  be saved, MCP resources.

### M4 Clip launching (in progress)
- The engine launches clips and scenes on a quantisation grid: bars or note values (triplets
  too, or none), following the time signature; 1 bar by default
  ([spec](docs/spec/clip-launching.md)).
- One clip per track; a clip loops until it is stopped or replaced, a clip without a loop plays
  once. Scenes launch their row, empty slots with a stop button stop their track, and a scene
  tempo changes the tempo at the launch.
- A track that launched a clip leaves its arrangement clips until "back to arrangement".
- Warped launched clips are time-stretched like arrangement clips, and re-pitched after a tempo
  change.
- Transport commands (`clip.launch`, `scene.launch`, `track.stop`, `launcher.*`,
  `transport.play`/`stop`) reach the audio thread through a lock-free queue:
  `loopwerk-cli play --launcher` takes them as typed lines, `loopwerk-cli mcp --audio` from
  agents, and `project_render` renders a script of them offline.
- Still to do: follow actions, launch modes, per-clip quantisation, reading the global
  quantisation from a Live Set, and checking the behaviour marked as hypotheses in the spec
  against Live.

### M5 Audio (in progress)
- Audio files (WAV, AIFF, FLAC, MP3, Ogg Vorbis) are decoded in parallel when a set opens.
- Audio clips play with clip gain, loops and fades; unwarped clips also with transpose.
- Warped clips are **time-stretched in real time** along their warp markers and keep their
  pitch ([ADR 0003](docs/decisions/0003-time-stretching.md)). An 80 s set with 58 warped clips
  opens in 3.1 s and plays at about 20 % of one core.
- Still to do: transpose on warped clips; rendering stretched clips slightly ahead on a worker
  thread so heavy sets cannot crackle; a disk cache for the stretcher's analysis; Rubber Band as
  the `HighQuality` stretch mode.
