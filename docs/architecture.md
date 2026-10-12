---
description: How the crates, threads and commands fit together, from a file to the speakers, and where new code goes
read_when: adding a crate or feature, where does code go, threads, audio thread, data flow, layers, player, controller
type: system
status: current
last_verified: 2026-10-12
covers:
  - Cargo.toml
  - crates/*/Cargo.toml
  - devices/*/Cargo.toml
  - crates/engine/src/lib.rs
  - crates/session/src/lib.rs
  - apps/loopwerk-cli/src/play.rs
  - crates/output/src/lib.rs
  - crates/ui/src/lib.rs
  - crates/ui/src/app.rs
  - apps/loopwerk/src/main.rs
  - crates/automation/src/server.rs
---

# Architecture

How Loopwerk's parts fit together: the layers, how a project gets from a file to the speakers,
which threads exist and what each may do, and where new code goes. The rules behind it are in
[`AGENTS.md`](../AGENTS.md); the larger decisions in [`decisions/`](decisions/). This page
describes the code as it is; planned parts are marked **(planned)**. A change that makes this
page wrong updates it in the same pull request (AGENTS.md › Before you finish a task).

## Layers

A crate may depend only on crates in lower layers. Nothing below the UI depends on a GUI toolkit.

```text
L3  automation        output          ui                apps/loopwerk-cli, apps/loopwerk
      MCP server,       audio device    window, views      the CLI; the desktop app
      renders           (cpal)          (egui)
        |                 |               |
L2  session ----------------- media
      commands, undo,           finds and decodes
      the player                audio files
        |        \              |
L1  als           engine -------+
      Live Set     rendering, launcher, time-stretching
      reader       no file I/O
        \        /
L0        model                    dsp, device-api
      the project as pure data       devices/* (built-in devices)
```

The built-in devices sit apart: `dsp` and `device-api` depend on nothing in Loopwerk, and each
device crate under `devices/` depends only on those two ([ADR 0004](decisions/0004-built-in-devices.md)).
The engine does not run them yet (see *Planned parts*).

| Crate | Owns | Depends on |
|---|---|---|
| `model` | The DAW-neutral project: tracks, mixer channels, a pool of clip contents, clips that place them, the launcher grid ([ADR 0001](decisions/0001-daw-neutral-model.md)). Validation. No behaviour beyond that. | nothing |
| `stretch` | Time-stretching and pitch-shifting DSP for interleaved audio: a pure-Rust port of Signalsmith Stretch (MIT), behind every stretch mode except `Repitch` ([ADR 0003](decisions/0003-time-stretching.md)). | nothing (rustfft) |
| `als` | Reading Live Sets into `model`, with warnings for what it skips ([spec](spec/als-format.md)). One crate per format later. | model |
| `engine` | Turning a project into audio: note voices, audio clips, real-time time-stretching ([ADR 0003](decisions/0003-time-stretching.md)), the clip launcher ([spec](spec/clip-launching.md)), device chains on tracks, with an instrument playing its track's notes ([ADR 0004](decisions/0004-built-in-devices.md)), plug-in slots in those chains that run a hosted plug-in's audio half (`PluginProcessor`) once per block with its notes, until the block's deadline ([ADR 0006](decisions/0006-plug-in-hosting.md)), latency compensation across tracks, group and master gain. Never touches files and never loads a plug-in. | model, device-registry |
| `media` | Finding a project's audio files and decoding them (symphonia) on all cores into an `AudioPool`. | engine |
| `session` | An open project with its undo history (`Session`), the open projects with one active, like tabs (`Projects`, [ADR 0007](decisions/0007-several-open-projects.md)), the command registry ([ADR 0002](decisions/0002-commands.md)), the `Player` that connects a session to a running engine, and the project's running plug-ins (`rack`: each plug-in device found in the catalog, started with its saved state off the audio thread, handed to the engine, watched and restarted; `start_offline` for renders). | model, engine, als, plugins |
| `automation` | The MCP server over the open projects, file roots for agents, offline WAV rendering ([`mcp.md`](mcp.md)). | session, engine, media |
| `output` | The audio device: plays a `Renderer` on the default output (cpal), and `open_player`, a `Player` whose stream lives on its own thread. The only code that opens an audio device. | engine, session |
| `ui` | The user interface ([ADR 0009](decisions/0009-ui-toolkit-and-look.md)): the main window with the Session and Arrangement views and the Settings page, drawn with egui from the project and the `LauncherMonitor`; every change is a command. Design tokens in `theme.rs`; the colours are the theme in use (`themes.rs` keeps the list). No windowing or GPU code, so tests drive it headless (egui_kittest). | engine, session |
| `loopwerk-cli` | The command line: render, play, mcp. | all of the above |
| `loopwerk` | The desktop app: opens a project (argument or a dropped `.als`), decodes its audio, plays it stopped through `output` and runs `ui` in an eframe window; loads and saves the settings, keymap and theme files (a theme file saved elsewhere is read again; theme backups and the theme template are zips in `exports/`, and theme files or zips dropped on the window are added). | session, media, output, ui |
| `dsp` | Signal-processing blocks for devices: dB conversions, parameter smoothing, biquad filters, a DC blocker, Linkwitz-Riley crossovers (24 and 48 dB/octave), LFO waveforms, waveshapers and a 2× oversampler. Real-time safe, panic-free on any input. | nothing |
| `device-registry` | Every built-in device by its stable id: makes the device a project's `DeviceKind::Builtin` names, and lists them. | device-api, devices/* |
| `device-api` | The `Device` trait, `DeviceInfo` (an effect or an instrument) and `ParamInfo` (plain values in their unit, conversion to knob travel), `NoteEvent` for instruments, and the contract tests every device runs (`conformance::check`, with note checks for instruments). | nothing |
| `ipc` | Audio blocks between the engine and a plug-in host process ([ADR 0006](decisions/0006-plug-in-hosting.md)): a shared-memory region with each block's notes and controllers (`Event`, up to 1024 a block), one block in flight, a doorbell per direction (a futex on Linux, events on Windows, yielding elsewhere for now) and a deadline; a crashed or hung host costs missed blocks, never a stall. One of the two crates allowed `unsafe`. Linked into the engine's process through `plugins`. | nothing |
| `plugins` | The engine's side of plug-in hosting: `Plugin::start` loads an instance into a `loopwerk-plugin-host` process (the one its group shares, `Launch::group`, or one of its own) and returns a control half (health, ping, state, parameters, restart) and an audio half (`PluginAudio::process`, real-time safe). Control messages are JSON lines over a local socket (`protocol`). A host that exits is *crashed*; one that leaves 32 blocks in a row unanswered is *hung*; `restart` brings it back with its last saved state. `scan` and `scan_pool` find installed CLAP and VST3 plug-ins (standard folders, `CLAP_PATH`, folders the user adds), read VST3 bundles' `moduleinfo.json` without loading them and load the rest in a few long-lived `--scan-worker` processes with per-file timeouts, into a catalog that can be saved. `PluginAudio::process_with_events` sends an instrument its notes. `offline::render` runs audio (and `render_with_events` notes) through a plug-in without a deadline (`loopwerk-cli plugin run`, `plugin run-device`). The session starts a project's plug-ins with it and hands their audio halves to the engine. | ipc |
| `plugin-host` | The `loopwerk-plugin-host` executable: answers control messages and serves plug-ins' main-thread callbacks on its main thread, and runs several instances, each with its own link and audio thread. Loads CLAP plug-ins (`clap:<file>[#<id>]`, through `clack-host`) and VST3 plug-ins (`vst3:<bundle>[#<class id>]`, through the `vst3` bindings), stereo in and out (no input for an instrument), notes and controllers, parameters, editors in a window of their own (Windows, X11 on Linux), with state and latency, and test plug-ins (`test:clap`, `test:vst3`, `test:gain`, `test:crash-after:<n>`, `test:hang-after:<n>`). `--scan <file>` and `--scan-worker` describe plug-in files for the scanner. The second crate allowed `unsafe`: opening plug-in files and the VST3 COM calls, each block documented. | ipc, plugins, clack |
| `devices/utility` | The utility device: gain, balance, width, mid/side, mono, bass mono (with listening to the low band alone), phase inversion, channel selection, DC filter, mute. | dsp, device-api |
| `devices/auto-pan` | The auto-pan device: an LFO (six shapes, shaping, inversion) that pans or, with no stereo phase, does tremolo; free in hertz or synced to the song position. | dsp, device-api |
| `devices/dj-eq` | The DJ EQ: low, mid and high bands with gains and kills, movable crossovers at 24 or 48 dB/octave, flat when every gain is at 0 dB. | dsp, device-api |
| `devices/vinyl-distortion` | Vinyl distortion: tracing (even harmonics in a band), pinch (odd harmonics, out of phase between the sides or mono) and crackle; shaped at twice the sample rate, 31 frames of latency. | dsp, device-api |
| `devices/overdrive` | Pedal-style overdrive: a band into a biased, asymmetric soft curve at twice the sample rate, then tone, dynamics, dry/wet and output; 31 frames of latency. | dsp, device-api |
| `devices/fm-synth` | The FM synth, an instrument: six operators, 32 algorithms, sixteen voices; the engine is a port of MSFA (the engine in Dexed, Apache-2.0) that matches it sample for sample; reads and writes DX7 voice files ([spec](spec/dx7-voice-format.md)). | device-api |

## From a file to the speakers

```text
.als ──als::read_file──> Project ──media::load_project_audio──> AudioPool
                            │                                       │
                            └──────────> Renderer::with_audio <─────┘
                                              │ (all allocation happens here)
                                              ▼
                       audio thread: Renderer::process(block), forever
```

1. **Import.** A format crate reads the file into a `Project`. Anything it does not understand
   becomes a warning; the project still opens.
2. **Load audio.** `media` decodes every file the project's audio clips use. The result, an
   `AudioPool`, holds the samples in memory, so playback never reads the disk.
3. **Build the renderer.** `Renderer::with_audio` turns the project into flat, sorted lists the
   audio thread can walk without searching: note events, audio events, one stretch voice per
   warped clip placement (two stretchers each, so a fresh one takes over before every strong
   onset and transients stay on the grid; ADR 0003), the prepared launcher clips, the
   per-track gains (group and master chain included) and a device chain for each track whose
   channel holds built-in devices, made through `device-registry` and prepared (ADR 0004), with
   a slot for each plug-in device the session knows of (ADR 0006), and the delay lines of
   latency compensation. Every buffer the renderer will ever need
   is allocated here. The player takes the stretch worker (`Renderer::take_stretch_worker`) and
   runs it on its own thread.
4. **Play.** `Renderer::process` fills one block of interleaved stereo. It never allocates,
   locks, blocks or does I/O. Offline rendering (`render_offline*`, `render_launches`) calls the
   same `process` in a loop, so an offline render is the test oracle for playback, and the output
   does not depend on the block size.

## Threads

| Thread | Runs | May allocate, lock, do I/O |
|---|---|---|
| Main / session | The CLI, the desktop app's window (egui, one frame at a time), or the MCP server's tokio runtime. Owns the `Session` (the MCP server: all open `Projects`), runs commands, imports and loads audio, builds the `Renderer` and, after each edit, a new graph for it (ADR 0005). | yes |
| Audio | The device callback: `Renderer::process`, sample conversion, setting atomics. | **no** (AGENTS.md rule 3) |
| Stretch worker | One thread, owned by the `Controller`, running a `StretchWorker` per graph: renders the arrangement's stretched clips 0.5 s ahead into a lock-free ring per clip (ADR 0003). A new graph's worker joins it, an old one leaves once its graph is freed. | only when a graph's worker joins; never locks |
| Audio owner | In the desktop app and over MCP, one thread per open project holds the output stream (`output::open_player`) and closes it when the session drops its `Player`. | yes, but does nothing while playing |
| Decode workers | Scoped threads in `media` while a project loads. | yes |
| Plug-in host processes | Not threads of Loopwerk's process: `loopwerk-plugin-host` processes, by default one per plug-in file with every instance of it inside, each instance with an audio thread of its own (ADR 0006). Started by the session through `plugins`, on a short-lived thread per plug-in so loading never blocks the window; the audio thread waits for each at most until its block's deadline. | the host's own business; a crash or hang there costs Loopwerk missed blocks only |

The main thread and the audio thread share nothing mutable. They talk in two directions:

- **To the audio thread:** a `Controller` holds the producer end of a lock-free queue (`rtrb`)
  of `LaunchCommand`s (play, stop, launch clip or scene, stop column, quantisation, back to
  arrangement). `process` drains the queue at the start of each block. A second queue carries
  new graphs after edits, and a third takes the old graphs back to be freed on the main thread
  (ADR 0005).
- **From the audio thread:** a `LauncherMonitor` of atomics (running, beat, tempo, what each
  column plays and has queued, the playhead, stretch underruns), read by the `launcher.state`
  command and by `Controller::update` to pick the frame a new graph takes over at.

## Commands

Every user-visible action is a command in `session` with an id, typed parameters and a kind
([ADR 0002](decisions/0002-commands.md)). The CLI, the MCP server and later the UI all call
`Session::execute(id, params)`, or `Projects::execute` when several projects are open (it runs
`projects.*` itself and passes everything else to the active project); none of them changes a
project directly.

| Kind | What it touches | Undo |
|---|---|---|
| Read | Reads the project (or the monitor) and returns JSON. | none |
| Edit | Changes the session's `Project`. Validated afterwards; a failed or invalid edit restores the project. | one step, a snapshot |
| History | Undo and redo. | moves between snapshots |
| Transport | Sends a `LaunchCommand` to the attached `Player`; fails if no engine is playing. | none |
| Projects | Lists, switches and closes open projects (`Projects`); only the active one keeps its `Player`. | none |
| Plugins | Scans for installed plug-ins, lists them, chooses the folders scans look in and how a plug-in shares host processes, and finds the installed plug-in for each of a project's devices (`plugins.*`, on the `Library` that `Projects` holds; ADR 0006). | none |
| Plugin | Acts on one of the project's running plug-ins: opens or closes its editor, restarts it (`device.openEditor`, `device.closeEditor`, `device.restart`). | none |

**Edits while playing** ([ADR 0005](decisions/0005-edits-while-playing.md)): after an Edit or
History command changes the project, the session calls `Player::update`. The controller builds a
new `Graph` (everything the renderer derives from the project) on the main thread and the audio
thread swaps it in at the start of the next block (0.15 s ahead when stretched arrangement clips
need their voices moved first), carrying over the playhead (on the same beat when the tempo
changed), the transport, sounding notes (which take their track's new mix), device chains and
the launcher's playing and queued clips. A device parameter is not a new graph: it goes to the
audio thread as a message, applied at the next block (ADR 0005, amendment).

**Plug-ins** (ADR 0006): `Projects::poll_plugins` (the window calls it every frame, commands
call it too) keeps the active project's running plug-ins in line with it. A new plug-in device
is looked up in the catalog and started with its saved state on a thread of its own; once it
runs, its audio half goes to the audio thread as a message (`Controller::send_plugin`) and takes
its slot, and every later graph takes it over from the one before; what it replaces goes back
to be dropped on the main thread. Its latency, once known, makes a new graph with the delays of
latency compensation. A plug-in that is not installed is *missing*: it has no slot and its
state stays in the project untouched. Offline renders start the plug-ins first and run the same
renderer without a deadline.

## Where new code goes

| You are adding | It goes in | Keep in mind |
|---|---|---|
| A field the project needs to store | `model`, then every reader and writer that should fill it | Neutral names; read ADR 0001 first |
| Something read from a Live Set | `als`, after it is written down in `docs/spec/` | Clean-room rules; a corpus test |
| Something that makes or changes sound | `engine`, prepared in the renderer's constructor | `process` never allocates; an offline render test |
| A way for the user or an agent to change something | a command in `session` | Typed params, undo, a test in `session/tests` |
| A new kind of file to decode or analyse | `media` | Input sizes are hostile; cap them |
| Something agents need | `automation`, on top of commands | File roots; `isError` for failed commands |
| A view or control in the UI | `ui`, drawn from `Grid`-like data built from the project and monitors | Dispatches commands; never edits the model directly; sizes and colours from `theme` and `docs/design/` |
| A built-in instrument or effect | a crate under `devices/`, blocks in `dsp` | ADR 0004; `conformance::check` in a test; import mapping in the format crate |

## Planned parts

- **Projects:** copying clips, tracks and devices between open projects, and switching without
  reopening the audio output ([ADR 0007](decisions/0007-several-open-projects.md) › Not decided
  yet).
- **Patch devices:** community devices as Pure Data patches run in the plug-in host process,
  with a command channel ([ADR 0008](decisions/0008-patch-devices.md), proposed).
- **Render-ahead for launched clips:** arrangement clips are stretched ahead on the worker;
  launched clips' stretch voices still run on the audio thread.
- **Native project format:** so edits can be saved; written and read by its own crate.
- **Device automation and more chains:** automation of device parameters, and chains on group,
  return and master channels ([ADR 0004](decisions/0004-built-in-devices.md) › Not decided
  yet). Chains on tracks run, with plug-ins and latency compensation, and `device.*` commands
  edit them.
- **Plug-ins:** consecutive plug-ins of a chain that share a host process in one round trip per
  block; plug-in parameters as automation; editors embedded in Loopwerk's window (ADR 0006).
- **UI:** the browser, the detail area (device chain, clip editor), meters, and the commands
  they still need (arm, sends) (M8).
