# Architecture

How Loopwerk's parts fit together: the layers, how a project gets from a file to the speakers,
which threads exist and what each may do, and where new code goes. The rules behind it are in
[`AGENTS.md`](../AGENTS.md); the larger decisions in [`decisions/`](decisions/). This page
describes the code as it is; planned parts are marked **(planned)**. A change that makes this
page wrong updates it in the same pull request (AGENTS.md › Before you finish a task).

## Layers

A crate may depend only on crates in lower layers. Nothing below the UI depends on a GUI toolkit.

```text
L3  automation            apps/loopwerk-cli          (planned) UI
      MCP server, renders     render, play, mcp
        |                       |
L2  session ----------------- media
      commands, undo,           finds, decodes, analyses
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
| `als` | Reading Live Sets into `model`, with warnings for what it skips ([spec](spec/als-format.md)). One crate per format later. | model |
| `engine` | Turning a project into audio: note voices, audio clips, real-time time-stretching ([ADR 0003](decisions/0003-time-stretching.md)), the clip launcher ([spec](spec/clip-launching.md)), group and master gain. Never touches files. | model |
| `media` | Finding a project's audio files, decoding them (symphonia) on all cores and analysing the ones warped clips play, into an `AudioPool`. | engine |
| `session` | One open project, its undo history, the command registry ([ADR 0002](decisions/0002-commands.md)) and the `Player` that connects a session to a running engine. | model, engine, als |
| `automation` | The MCP server over a session, file roots for agents, offline WAV rendering ([`mcp.md`](mcp.md)). | session, engine, media |
| `loopwerk-cli` | The command line, and the only code that opens an audio device (cpal). | all of the above |
| `dsp` | Signal-processing blocks for devices: dB conversions, parameter smoothing, biquad filters, a DC blocker, Linkwitz-Riley crossovers (24 and 48 dB/octave), LFO waveforms. Real-time safe, panic-free on any input. | nothing |
| `device-api` | The `Device` trait, `DeviceInfo` and `ParamInfo` (plain values in their unit, conversion to knob travel), and the contract tests every device runs (`conformance::check`). | nothing |
| `devices/utility` | The utility device: gain, balance, width, mid/side, mono, bass mono, phase inversion, channel selection, DC filter, mute. | dsp, device-api |
| `devices/auto-pan` | The auto-pan device: an LFO (six shapes, shaping, inversion) that pans or, with no stereo phase, does tremolo; free in hertz or synced to the song position. | dsp, device-api |
| `devices/dj-eq` | The DJ EQ: low, mid and high bands with gains and kills, movable crossovers at 24 or 48 dB/octave, flat when every gain is at 0 dB. | dsp, device-api |

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
2. **Load audio.** `media` decodes every file the project's audio clips use and computes the
   stretcher's analysis for files that warped clips play. The result, an `AudioPool`, holds the
   samples in memory, so playback never reads the disk.
3. **Build the renderer.** `Renderer::with_audio` turns the project into flat, sorted lists the
   audio thread can walk without searching: note events, audio events, one stretch voice per
   warped clip placement, the prepared launcher clips and the per-track gains (group and master
   chain included). Every buffer the renderer will ever need is allocated here.
4. **Play.** `Renderer::process` fills one block of interleaved stereo. It never allocates,
   locks, blocks or does I/O. Offline rendering (`render_offline*`, `render_launches`) calls the
   same `process` in a loop, so an offline render is the test oracle for playback, and the output
   does not depend on the block size.

## Threads

| Thread | Runs | May allocate, lock, do I/O |
|---|---|---|
| Main / session | The CLI, or the MCP server's tokio runtime. Owns the `Session`, runs commands, imports and loads audio, builds the `Renderer`. | yes |
| Audio | The device callback: `Renderer::process`, sample conversion, setting atomics. | **no** (AGENTS.md rule 3) |
| Audio owner | Over MCP, one thread per open project holds the output stream and closes it when the session drops its `Player`. | yes, but does nothing while playing |
| Decode workers | Scoped threads in `media` while a project loads. | yes |

The main thread and the audio thread share nothing mutable. They talk in two directions:

- **To the audio thread:** a `Controller` holds the producer end of a lock-free queue (`rtrb`)
  of `LaunchCommand`s (play, stop, launch clip or scene, stop column, quantisation, back to
  arrangement). `process` drains the queue at the start of each block.
- **From the audio thread:** a `LauncherMonitor` of atomics (running, beat, tempo, what each
  column plays and has queued), read by the `launcher.state` command.

## Commands

Every user-visible action is a command in `session` with an id, typed parameters and a kind
([ADR 0002](decisions/0002-commands.md)). The CLI, the MCP server and later the UI all call
`Session::execute(id, params)`; none of them changes a project directly.

| Kind | What it touches | Undo |
|---|---|---|
| Read | Reads the project (or the monitor) and returns JSON. | none |
| Edit | Changes the session's `Project`. Validated afterwards; a failed or invalid edit restores the project. | one step, a snapshot |
| History | Undo and redo. | moves between snapshots |
| Transport | Sends a `LaunchCommand` to the attached `Player`; fails if no engine is playing. | none |

**Current limit:** an edit changes the session's project, but a renderer that is already playing
keeps the project it was built from. Edits are heard the next time a renderer is built (a new
`play`, `project_render`). Making edits reach a playing engine is the next step for the engine:
build a new renderer, or the changed part of it, off the audio thread and swap it in atomically,
as rule 3 describes. **(planned)**

## Where new code goes

| You are adding | It goes in | Keep in mind |
|---|---|---|
| A field the project needs to store | `model`, then every reader and writer that should fill it | Neutral names; read ADR 0001 first |
| Something read from a Live Set | `als`, after it is written down in `docs/spec/` | Clean-room rules; a corpus test |
| Something that makes or changes sound | `engine`, prepared in the renderer's constructor | `process` never allocates; an offline render test |
| A way for the user or an agent to change something | a command in `session` | Typed params, undo, a test in `session/tests` |
| A new kind of file to decode or analyse | `media` | Input sizes are hostile; cap them |
| Something agents need | `automation`, on top of commands | File roots; `isError` for failed commands |
| A new front end (UI) | its own crate on top of `session` | Dispatches commands; never edits the model directly |
| A built-in instrument or effect | a crate under `devices/`, blocks in `dsp` | ADR 0004; `conformance::check` in a test; import mapping in the format crate |

## Planned parts

- **Edits while playing:** see *Current limit* above.
- **Render-ahead for stretch voices:** warped clips rendered slightly ahead on a worker thread
  and handed to the audio thread through lock-free rings, so heavy sets cannot crackle
  (ADR 0003, consequences).
- **Native project format:** so edits can be saved; written and read by its own crate.
- **Devices in the engine:** per-track buffers and device chains in the renderer, a registry
  from `DeviceKind::Builtin` ids to the device crates, parameter values in the model, parameter
  commands and automation ([ADR 0004](decisions/0004-built-in-devices.md) › Not decided yet).
  The devices themselves (`dsp`, `device-api`, `devices/*`) exist and are tested on their own.
- **Plug-in host:** third-party plug-ins in their own processes, never in the engine's
  (AGENTS.md rule 5). The engine talks to them over shared-memory audio buffers with a per-block
  deadline; plug-in state and parameters go over messages. A crashing plug-in is reported and
  restarted without losing the project. An ADR comes first (M6).
- **UI:** Session and Arrangement views as a crate on top of `session`, using the same commands
  and reading engine state through monitors like `LauncherMonitor` (M8).
