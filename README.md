# Suhina

An open-source, clip-launching DAW in pure Rust, comparable in workflow to Ableton Live.

Suhina was called Loopwerk until October 2026; settings kept under the old name carry over on the first start. <!-- rename_suhina: keep -->

> **Status:** pre-alpha. Suhina opens Live Sets, plays and renders them (MIDI as sine tones for
> now, audio clips with real-time time-stretching), and lets agents edit them over MCP. The
> desktop app shows the Session view so far. See [`ROADMAP.md`](ROADMAP.md) for what works and what comes next.

Suhina is an independent clean-room project. It is not affiliated with, sponsored by or
endorsed by Ableton AG. Ableton and Live are trademarks of Ableton AG, named here only to describe
comparable workflows and file compatibility. Suhina contains no code or content from Ableton
products; see [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.

## Goals

- A real-time audio engine that never glitches, with offline rendering on the same code path.
- Session-style clip launching with quantisation, plus a linear arrangement.
- Reading `.als` projects you made yourself; later other formats such as DAWproject and `.flp`,
  so projects are not locked to one program.
- CLAP and VST3 plug-in hosting, sandboxed outside Suhina's process: a crashing plug-in never takes the project down.
- Every action available headlessly: CLI and MCP for automation and agents.

## Downloads

Tagged versions build the headless command-line tool `suhina-cli` for Linux (x86_64), Windows
(x64) and macOS (universal) and attach them to a GitHub Release. The binaries are not signed yet,
so macOS and Windows warn before running them. On Linux they need the ALSA library (`libasound2`).
How a release is made: [`docs/releasing.md`](docs/releasing.md).

## Try it

```sh
# Open a Live Set in the desktop app and launch its clips and scenes with the mouse; the
# plug-in host is built first, since the app runs plug-ins in it (scan in Settings › Plug-ins)
cargo build --release -p suhina-plugin-host
cargo run --release -p suhina -- path/to/set.als

# Play a Live Set on the default audio device (release builds play heavy sets smoothly)
cargo run --release -p suhina-cli -- play path/to/set.als

# Play one launcher scene instead of the arrangement
cargo run --release -p suhina-cli -- play path/to/set.als --scene 2

# Launch clips and scenes by typing commands (see below)
cargo run --release -p suhina-cli -- play path/to/set.als --launcher

# Render to a 32-bit float WAV
cargo run --release -p suhina-cli -- render path/to/set.als -o out.wav

# Play every MIDI track on the built-in FM synth, with voice 3 of a DX7 bank (.syx)
cargo run --release -p suhina-cli -- play --demo --fm --voices path/to/bank.syx --voice 3

# List the voices in a DX7 voice or bank file
cargo run --release -p suhina-cli -- fm-synth voices path/to/bank.syx

# Serve the Model Context Protocol for agents (see docs/mcp.md)
cargo run --release -p suhina-cli -- mcp --read-root path/to/projects --write-root path/to/renders
```

`--demo` instead of a file uses a small built-in project. `--fm` works with `play` and `render`,
for the demo or any set; without `--voices` the synth plays its initial voice, a plain sine.

With `--launcher` the set starts stopped and Suhina reads one command per line:

| Type | Does |
|---|---|
| `scene 2` | launch scene 2 |
| `clip Bass 1` | launch the clip of track *Bass* in scene 1 |
| `stop Bass`, `stopall` | stop one track, or every track |
| `play`, `stop` | start or stop the transport |
| `quant 1/4` | launch quantisation: `none`, `8 bars` … `1 bar`, or a note value from `1/2` to `1/32`, `T` for triplets (`1/8T`) |
| `back` | return every track to the arrangement |
| `state`, `grid` | what is playing and queued; the scenes and clips |
| `metronome on`, `metronome on -12`, `metronome off` | a click on every beat, higher on the bar; level in dB (−6 by default) |
| `tempo 100`, `vol Bass -6`, `mute Bass`, `unmute Bass` | edits, heard while playing |
| `undo`, `redo` | undo or redo the last edit |
| `track.setMute {"track": 1, "mute": true}` | any command by id, with JSON parameters |
| `help`, `quit` | |

## Building

Linux builds talk to the sound card through ALSA, so its development files and `pkg-config`
must be installed before building:

| Distribution family | Packages |
|---|---|
| Debian, Ubuntu, Mint | `libasound2-dev pkg-config` |
| Fedora, RHEL | `alsa-lib-devel pkgconf-pkg-config` |
| Arch, Manjaro, EndeavourOS | `alsa-lib pkgconf` |

Building the whole workspace also builds `suhina-plugin-host`, the program the desktop app
runs plug-ins in; it has to sit next to `suhina`:

```sh
cargo build --release
cargo test --workspace
```

## Documentation

| | |
|---|---|
| [`docs/INDEX.md`](docs/INDEX.md) | Every document, what it is for, and the code it describes |
| [`docs/commands.md`](docs/commands.md) | Every command the CLI, MCP server and UI run, with its parameters |
| [`ROADMAP.md`](ROADMAP.md) | Milestones, what works now, what is next |
| [`AGENTS.md`](AGENTS.md) | Rules for contributors and agents, and the crate map |
| [`docs/architecture.md`](docs/architecture.md) | How the parts fit: layers, data flow, threads, commands, where new code goes |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Clean-room rules, licences, sign-off (DCO), AI-assisted contributions |
| [`IP.md`](IP.md) | Reporting a rights or clean-room problem, and how material is removed |
| [`docs/decisions/`](docs/decisions/) | Architecture decisions: the DAW-neutral model, commands, time-stretching |
| [`docs/spec/`](docs/spec/) | The Live Set format as observed, and how to author test sets |
| [`docs/mcp.md`](docs/mcp.md) | The MCP server: tools, file roots, errors |

## Community

- Discord: <https://discord.gg/ccQWu8rRcf>

## License

Suhina is free software, licensed under the [GNU General Public License v3.0 or later](LICENSE).
Contributions are licensed the same way.
