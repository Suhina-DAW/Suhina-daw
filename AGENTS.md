# AGENTS.md: guide for AI agents and contributors

Loopwerk is an open-source, native, clip-launching DAW written in **Rust only**, comparable in
workflow to Ableton Live. The product name is always written **Loopwerk** in user-facing text;
machine names stay lowercase (`loopwerk-*` crates, binaries, file names). Read this file first,
then [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/architecture.md`](docs/architecture.md)
(layers, data flow, threads, and where new code goes).

## 1. Golden rules

1. **Clean-room (outranks everything).** Follow [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.
   Never disassemble or decompile proprietary DAW or plug-in binaries, never use leaked code, never
   commit content from commercial products. Knowledge comes from observation, self-authored files
   and public documentation, and is written down in `docs/spec/` before it is implemented.
2. **Never crash.** Non-test code never panics: no `unwrap()`, `expect()`, `panic!`,
   `unreachable!`, `todo!` or `unimplemented!`. Return the crate's error type. Input-derived
   numbers are hostile: use `get()` and `checked_*`, cap allocations sized by input. No `unsafe`
   (`unsafe_code = "forbid"` workspace-wide).
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
   comes to depend on.
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
  engine   L1 offline/real-time rendering of notes and audio clips, real-time time-stretching (ADR 0003)
           and the clip launcher; no file I/O
  als      L1 Live Set (.als) reader into model; later one crate per format (dawproject, flp)
  media    L2 finds, decodes and analyses a project's audio files (symphonia) into the engine's AudioPool
  session  L2 open project, undo history, the command registry (ADR 0002) and the attached player
  automation L3 MCP server (rmcp) over a session, file roots, WAV rendering (docs/mcp.md)
apps/
  loopwerk-cli   headless CLI: render, play, mcp
docs/
  architecture.md  how the crates, threads and commands fit together
  decisions/  architecture decision records; read 0001 before changing the model
  spec/       clean-room specs written from observation (formats, behaviour)
scripts/   research helpers (e.g. extracting and diffing .als files)
```

A crate may depend only on lower layers. Nothing below the UI layer may depend on a GUI toolkit.

## 3. Before you finish a task

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
