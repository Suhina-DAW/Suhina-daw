# Loopwerk

An open-source, clip-launching DAW in pure Rust, comparable in workflow to Ableton Live.

> **Status:** pre-alpha. Nothing to use yet.

Loopwerk is an independent clean-room project. It is not affiliated with, sponsored by or
endorsed by Ableton AG. Ableton and Live are trademarks of Ableton AG, named here only to describe
comparable workflows and file compatibility. Loopwerk contains no code or content from Ableton
products; see [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.

## Goals

- A real-time audio engine that never glitches, with offline rendering on the same code path.
- Session-style clip launching with quantisation, plus a linear arrangement.
- Reading `.als` projects you made yourself; later other formats such as DAWproject and `.flp`,
  so projects are not locked to one program.
- CLAP plug-in hosting.
- Every action available headlessly: CLI and MCP for automation and agents.

## Downloads

Tagged versions build the headless command-line tool `loopwerk-cli` for Linux (x86_64), Windows
(x64) and macOS (universal) and attach them to a GitHub Release. The binaries are not signed yet,
so macOS and Windows warn before running them. On Linux they need the ALSA library (`libasound2`).
How a release is made: [`docs/releasing.md`](docs/releasing.md).

## Building

On Linux, install the ALSA headers first (`libasound2-dev` on Debian and Ubuntu,
`alsa-lib-devel` on Fedora).

```sh
cargo build --release
cargo test --workspace
```

## License

Loopwerk is free software, licensed under the [GNU General Public License v3.0 or later](LICENSE).
Contributions are licensed the same way.
