# AGENTS.md: guide for AI agents and contributors

Loopwerk is an open-source, native, clip-launching DAW written in **Rust only**, comparable in
workflow to Ableton Live. The product name is always written **Loopwerk** in user-facing text;
machine names stay lowercase (`loopwerk-*` crates, binaries, file names). Read this file first,
then [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/architecture.md`](docs/architecture.md)
(layers, data flow, threads, and where new code goes).

**Find documentation through [`docs/INDEX.md`](docs/INDEX.md)** before reading code: it lists
every document with the words it is read for (*Read when*), and maps code paths to the documents
that describe them (*Code and its documents*). Read only the documents that match your task.

## 1. Golden rules

1. **Clean-room (outranks everything).** Follow [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.
   Never disassemble or decompile proprietary DAW or plug-in binaries, never use leaked code, never
   commit content from commercial products. Knowledge comes from observation, self-authored files
   and public documentation, and is written down in `docs/spec/` before it is implemented.
2. **Never crash.** Non-test code never panics: no `unwrap()`, `expect()`, `panic!`,
   `unreachable!`, `todo!` or `unimplemented!`. Return the crate's error type. Input-derived
   numbers are hostile: use `get()` and `checked_*`, cap allocations sized by input. No `unsafe`
   (`unsafe_code = "forbid"` workspace-wide), except in the two crates plug-in hosting needs,
   `crates/ipc` and `crates/plugin-host` ([ADR 0006](docs/decisions/0006-plug-in-hosting.md)):
   there every `unsafe` block is small, tested and explained in a `// SAFETY:` comment, and the
   crate offers a safe API.
3. **Real-time safety.** Code reachable from the audio callback never allocates, frees, locks,
   blocks, logs or does I/O. Hand data to it through lock-free queues (`rtrb`) and swap whole
   graphs atomically.
4. **One render path.** Offline rendering and real-time playback run the same engine code, so
   offline renders are the test oracle for the engine.
5. **Plug-ins run sandboxed, out of process.** Third-party plug-ins (CLAP, VST3, later bridged
   Windows plug-ins) never run inside the Loopwerk process. A crashing or hanging plug-in must
   not take down the engine or lose the user's project: the host reports it, keeps the project
   intact, and can restart it. Design the plug-in host boundary as IPC from the first line of
   plug-in code (shared-memory audio buffers, per-block sync with a deadline, plug-in state and
   parameters over messages); never add an in-process shortcut that the rest of the engine
   comes to depend on. The design is [ADR 0006](docs/decisions/0006-plug-in-hosting.md).
6. **Everything is a command.** User-visible behaviour is a command in the engine with an id,
   params and tests. The UI, CLI and MCP all dispatch the same commands.
7. **Tests are the gate.** Every change comes with tests: round-trip and malformed-input tests for
   formats, golden-file renders for DSP and engine behaviour.
8. **Other DAWs are named only where a format is handled.** Product names such as Ableton,
   Live or FL Studio may appear in format crates (`crates/als`), their tests, `docs/spec/` and
   `docs/decisions/`, to say which file format the code reads. The model, engine, CLI core and
   UI describe concepts neutrally ("a clip that belongs to one track", "a shared pattern").
   User-facing text may name a file format when it imports or exports it ("a Live Set to
   import"), never as Loopwerk's own branding.
9. **GPL-compatible dependencies only.** Loopwerk is GPL-3.0-or-later. Dependencies must be
   compatible with GPLv3: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL, GPL-3.0. Not GPL-2.0-only,
   not proprietary or "non-commercial" licences.

## 2. Workspace map

```text
crates/
  model    L0 DAW-neutral project as pure data (tracks, channels, clip contents, clips, launcher)
  stretch  L0 time-stretching and pitch-shifting DSP: a pure-Rust port of Signalsmith Stretch (MIT)
  engine   L1 offline/real-time rendering of notes and audio clips, real-time time-stretching (ADR 0003)
           and the clip launcher; no file I/O
  als      L1 Live Set (.als) reader into model; later one crate per format (dawproject, flp)
  locr     L1 Loopwerk's own project file (.locr, versioned JSON): reads and writes the model (ADR 0010)
  media    L2 finds and decodes a project's audio files (symphonia) into the engine's AudioPool;
           reads MIDI files, file metadata (tempo, key) and detects tempo
  library  L2 the browser's library: added folders, an index kept on disk and watched, search, pins,
           recent items, colour tags (ADR 0012)
  soundmap L2 the sample map: features of samples, layouts by how they sound (t-SNE), grids, kinds, the feature cache
  session  L2 open projects (tabs, ADR 0007), undo history, the command registry (ADR 0002), the
           attached player, each project's decoded audio and previews (ADR 0011)
  automation L3 MCP server (rmcp) over a session, file roots, WAV rendering (docs/mcp.md)
  output   L3 the audio device (cpal): plays an engine, opens a Player for a session
  ui       L3 the main window, the browser and the views (egui, ADR 0009); every change is a command
  dsp      L0 signal-processing blocks for built-in devices: smoothing, filters, crossovers, LFOs, shapers, oversampling (ADR 0004)
  device-api L0 the Device trait, parameter descriptions, contract tests every device runs
  ipc      L0 audio blocks to a plug-in host process: shared memory, block sync with a deadline (ADR 0006; may use unsafe)
  plugins  L1 the engine's side of plug-in hosting: start a host process, control protocol, health, state, restart; scanning; offline runs
  plugin-host  the loopwerk-plugin-host executable plug-ins run in (ADR 0006; may use unsafe): CLAP, VST3, test plug-ins
devices/
  utility  gain, balance, width, mono, bass mono, phase, channels, DC filter; depends on dsp, device-api
  auto-pan LFO panning and tremolo, free in Hz or synced to the song position
  dj-eq    three bands with kills, Linkwitz-Riley crossovers at 24 or 48 dB/octave
  vinyl-distortion  tracing (even), pinch (odd, out of phase) and crackle, oversampled
  overdrive  pedal-style: band, drive, tone, dynamics, dry/wet; oversampled
  compressor stereo-linked, soft knee, side-chain high-pass, attack/release, dry/wet
  limiter  brickwall peak limiter with 1.5 ms lookahead; never over the ceiling
apps/
  loopwerk-cli   headless CLI: render, play, mcp
  loopwerk       the desktop app (eframe): opens a project and shows it in the ui
docs/
  architecture.md  how the crates, threads and commands fit together
  decisions/  architecture decision records; read 0001 before changing the model, 0004 before a device
  spec/       clean-room specs written from observation (formats, behaviour, UI views)
  design/     UI design brief, component measurements and reference mockups (ADR 0009)
  INDEX.md    generated list of documents and the code each covers (scripts/docs.py)
scripts/   research helpers (e.g. extracting and diffing .als files) and docs.py (the docs index)
```

A crate may depend only on lower layers. Nothing below the UI layer may depend on a GUI toolkit.

## 3. Tasks

Work is tracked in the GitHub project **Loopwerk**
(<https://github.com/users/juhamikael/projects/12>), and nowhere else. New work, bugs found while
testing and follow-ups a change leaves open go there as items, written in English, with a
Status, a Priority (P0 to P2) and a Phase (the ROADMAP milestone). Work that a pull request will
close gets an issue, so the pull request can say `Closes #N`. Items imported from the earlier
board keep their old key in the title (`(LOOP-27)`); a pull request for one names that key.

## 4. Before you finish a task

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Keep the tree building at every step.

**Documentation is part of the change.** A pull request that changes how something works updates,
in the same pull request, every document that describes it, so the docs never describe code
that no longer exists:

| If you change | Update |
|---|---|
| A crate's role, a dependency between crates, threads, how commands or data flow | [`docs/architecture.md`](docs/architecture.md), and the crate map above |
| A decision recorded in an ADR | that ADR (status, consequences), or a new ADR that supersedes it |
| What is read from a format, or behaviour described in a spec | the spec in `docs/spec/` |
| Commands or MCP tools | [`docs/mcp.md`](docs/mcp.md) and the command list in [`ROADMAP.md`](ROADMAP.md) |
| What works for users | [`ROADMAP.md`](ROADMAP.md) › What works now, and [`README.md`](README.md) if it shows it |
| A rule in this file | this file, and [`CONTRIBUTING.md`](CONTRIBUTING.md) if it repeats it |

A limit or a planned part that a change resolves is removed from the docs in the same change.

```sh
python scripts/docs.py impact   # documents that cover code you changed but you have not updated
python scripts/docs.py index    # after adding a document or changing its front matter
python scripts/docs.py check    # what CI runs: front matter, covered paths, INDEX.md current
```

Every file under `docs/` starts with front matter (format in `scripts/docs.py`): `description`,
`read_when`, `type` (system, decision, spec, howto, plan), `status`, `last_verified` and
`covers`, the code paths it describes. Set `last_verified` to the day you checked the text
against the code; CI warns when it is over 90 days old. A new document lists in `covers` the code
it describes, so `impact` can point the next change to it.
