# AGENTS.md: guide for AI agents and contributors

Suhina is an open-source, native, clip-launching DAW written in **Rust only**, comparable in
workflow to Ableton Live. The product name is always written **Suhina** in user-facing text;
machine names stay lowercase (`suhina-*` crates, binaries, file names). Read this file first,
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
   Windows plug-ins) never run inside the Suhina process. A crashing or hanging plug-in must
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
   `docs/decisions/`, to say which file format the code reads, and in the design brief
   (`docs/design/brief.md`), to say what Suhina's look must not resemble. They also appear in
   [`docs/research/parity-live-12.md`](docs/research/parity-live-12.md) and the scripts under
   `scripts/parity/`, which measure what Suhina has against another DAW's public manual: status
   analysis of our own project, never content from that manual (its PDF, text, tables, screenshots
   and parameter lists are never committed; the scripts read a copy the user owns and write to
   `target/parity/`). ROADMAP and README
   describe Suhina's own devices and features under their own names; which device of another
   product imports into which is a spec ([`docs/spec/device-import.md`](docs/spec/device-import.md)).
   Product terms and set phrases of other DAWs are not used as generic words either: "drum sampler",
   not "drum rack"; "built-in effects and instruments", not "built-in devices". "Device" on its own
   is a generic word and stays (the `device.*` commands, `devices/`). The model, engine, CLI core and
   UI describe concepts neutrally ("a clip that belongs to one track", "a shared pattern").
   User-facing text may name a file format when it imports or exports it ("a Live Set to
   import"), never as Suhina's own branding.
9. **GPL-compatible dependencies only.** Suhina is GPL-3.0-or-later. Dependencies must be
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
  project-file L1 Suhina's own project file (.suhina, versioned JSON; .locr still read): reads and writes the model (ADR 0010)
  media    L2 finds and decodes a project's audio files (symphonia) into the engine's AudioPool;
           reads MIDI files, file metadata (tempo, key) and detects tempo
  library  L2 the browser's library: added folders, an index kept on disk and watched, search, pins,
           recent items, colour tags (ADR 0012)
  soundmap L2 the sample map: features of samples, layouts by how they sound (t-SNE), grids, kinds, the feature cache
  session  L2 open projects (tabs, ADR 0007), undo history, the command registry (ADR 0002), the
           attached player, each project's decoded audio and previews (ADR 0011)
  automation L3 MCP server (rmcp) over a session, file roots, WAV rendering (docs/mcp.md)
  output   L3 the audio device: lists hosts and devices (cpal; ASIO with the `asio` feature), opens the one
           chosen (WASAPI exclusive through `wasapi`), plays an engine in blocks of the chosen buffer
  ui       L3 the main window, the browser and the views (egui, ADR 0009); every change is a command
  dsp      L0 signal-processing blocks for built-in effects and instruments: smoothing, filters, crossovers, LFOs, shapers, oversampling (ADR 0004)
  device-api L0 the Device trait, parameter descriptions, contract tests every device runs
  device-sdk L0 builds and validates data-only `.sd` effect graphs, compiles DSP primitives and delayed feedback for real-time execution; shared node contracts and embedded custom UI canvases/images for authors and agents
  device-registry L1 every built-in effect or instrument by its stable id: makes and lists them (ADR 0004)
  ipc      L0 audio blocks to a plug-in host process: shared memory, block sync with a deadline (ADR 0006; may use unsafe)
  plugins  L1 the engine's side of plug-in hosting: start a host process, control protocol, health, state, restart; scanning; offline runs
  plugin-host  the suhina-plugin-host executable plug-ins run in (ADR 0006; may use unsafe): CLAP, VST3, test plug-ins
devices/
  utility  gain, balance, width, mono, bass mono, phase, channels, DC filter; depends on dsp, device-api
  auto-pan LFO panning and tremolo, free in Hz or synced to the song position
  dj-eq    three bands with kills, Linkwitz-Riley crossovers at 24 or 48 dB/octave
  vinyl-distortion  tracing (even), pinch (odd, out of phase) and crackle, oversampled
  overdrive  pedal-style: band, drive, tone, dynamics, dry/wet; oversampled
  compressor stereo-linked, soft knee, side-chain high-pass, attack/release, dry/wet
  fm-synth six operators, 32 algorithms, DX7 voice files; the engine is a port of MSFA (Apache-2.0)
  limiter  brickwall peak limiter with 1.5 ms lookahead; never over the ceiling
apps/
  suhina-cli   headless CLI: render, play, mcp, commands, audio, plugins, plugin, fm-synth
  suhina       the desktop app (eframe): opens a project and shows it in the ui
docs/
  architecture.md  how the crates, threads and commands fit together
  decisions/  architecture decision records; read 0001 before changing the model, 0004 before a device
  spec/       clean-room specs written from observation (formats, behaviour, UI views)
  design/     UI design brief, component measurements and reference mockups (ADR 0009)
  research/   status analysis of our own project: parity against another DAW's public manual
              (ADR 0016; an ordinary document whose marked parts scripts/parity/ keeps current)
              and its two figures
  INDEX.md    generated list of documents and the code each covers (scripts/docs.py)
  commands.md generated list of every command (suhina-cli commands --markdown)
third_party/  vendored source built behind a feature: the ASIO SDK's host part (GPL-3.0, `asio`)
scripts/   research helpers (e.g. extracting and diffing .als files), the parity measurement
           (parity/, reading a manual the user owns; its output is never committed), docs.py
           (the docs index), release_notes.py and discord_post.py (release notes, Discord posts)
```

A crate may depend only on crates in its own or a lower layer, never in a cycle. Nothing below
the UI layer may depend on a GUI toolkit.

### Crate names

Every package is named `suhina-*`; the directory has no prefix. The project had another name
until October 2026 (README); nothing new may use it, in names, text or code.

| Kind | Directory | Package (`Cargo.toml`) | In Rust code |
|---|---|---|---|
| Library crate | `crates/<name>` | `suhina-<name>` | `suhina_<name>` |
| Built-in effect or instrument | `devices/<name>` | `suhina-device-<name>` | `suhina_device_<name>` |
| App (binary) | `apps/<name>` | `suhina` or `suhina-<name>` | |

A new crate inherits the workspace settings (`version.workspace = true`, `edition`, `license`,
`rust-version`, `[lints] workspace = true`), is added to `[workspace.dependencies]` in the root
`Cargo.toml` (`suhina-<name> = { path = "crates/<name>" }`), and other crates depend on it with
`suhina-<name>.workspace = true`. Add it to the map above and to
[`docs/architecture.md`](docs/architecture.md). Environment variables start with `SUHINA_`.
CI runs `python scripts/rename_suhina.py --check`, which fails when the old name is left anywhere
in the tree (links to the repository and lines marked `rename_suhina: keep` excepted). A branch started
before the rename catches up as the script's docstring describes.

## 3. Tasks

Work is tracked in the GitHub project **Suhina**
(<https://github.com/orgs/suhina-daw/projects/1>), and nowhere else. New work, bugs found while
testing and follow-ups a change leaves open go there as items, written in English, with a
Status, a Priority (P0 to P2) and a Phase (the ROADMAP milestone). Work that a pull request will
close gets an issue, so the pull request can say `Closes #N`. An issue gets a type label and
area labels (`CONTRIBUTING.md` › Issues lists them); a larger piece split into issues is an
`epic`, and repository, build and release work is a `chore`. Items imported from the earlier
board keep their old key in the title (`(LOOP-27)`); a pull request for one names that key.
Before starting, claim the item the way [`CONTRIBUTING.md`](CONTRIBUTING.md) › *Working on an issue*
describes — no open pull request for it, `Backlog` or `Ready` on the board, a comment with the plan —
for a person and for an agent alike.

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
| MCP tools, or how agents use commands | [`docs/mcp.md`](docs/mcp.md) (the command list, [`docs/commands.md`](docs/commands.md), is generated) |
| What works for users | [`ROADMAP.md`](ROADMAP.md) › What works now, and [`README.md`](README.md) if it shows it |
| What the parity measurement says (a milestone that lands, a feature that appears or is dropped) | the coverage of the chapters it changes in `scripts/parity/score.py`, then `python scripts/parity/update_report.py` for the marked parts of [`docs/research/parity-live-12.md`](docs/research/parity-live-12.md); its prose is edited in that file like any other document |
| A rule in this file | this file, and [`CONTRIBUTING.md`](CONTRIBUTING.md) if it repeats it |

A limit or a planned part that a change resolves is removed from the docs in the same change.

**Write documents so parallel pull requests do not collide.** In a list that several changes add
to (ROADMAP's *What works now*, the crate map), add a bullet or a row of your own rather than
extending someone else's sentence. Leave generated files out of the change:
[`docs/INDEX.md`](docs/INDEX.md) (from the documents' front matter) and
[`docs/commands.md`](docs/commands.md) (from the command registry) are regenerated on `main` after
every merge by [`.github/workflows/docs-generated.yml`](.github/workflows/docs-generated.yml), and
a pull request that edits one of them fails CI rather than losing the edit silently.

The parity report, its figures and the scripts that produce them are the exception, and are
described in the report itself: the **report** is an ordinary document that a pull request edits
freely, except for the parts between its `parity:begin` and `parity:end` markers, which
`scripts/parity/update_report.py` writes from `scripts/parity/score.py` and
`scripts/parity/devices.json` (CI fails when they are out of date); the **figures** are rendered on
`main` like the other generated files.

```sh
python scripts/docs.py impact   # documents that cover code you changed but you have not updated
                                # (a notice only, for files three or more documents cover)
python scripts/docs.py check    # what CI runs: front matter, covered paths, links and #anchors
python scripts/docs.py index    # prints the index with your documents in it; writes nothing
```

Every file under `docs/` starts with front matter (format in `scripts/docs.py`): `description`,
`read_when`, `type` (system, decision, spec, howto, plan), `status`, `last_verified` and
`covers`, the code paths it describes. Set `last_verified` to the day you checked the text
against the code; CI warns when it is over 90 days old. A new document lists in `covers` the code
it describes, so `impact` can point the next change to it.
