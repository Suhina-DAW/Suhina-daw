---
description: Version 1 of the data-only Suhina device graph format, SDK, track loading, and generated controls
read_when: .sd, Suhina device, device graph, device SDK, community effect, visual node, device UI
type: spec
status: current
last_verified: 2026-10-14
covers:
  - crates/device-sdk/Cargo.toml
  - crates/device-sdk/src/lib.rs
  - crates/device-sdk/examples/gain.rs
  - crates/model/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/session/src/devices.rs
  - crates/engine/src/chain.rs
  - crates/ui/src/detail_view.rs
  - crates/media/src/info.rs
---

# Suhina device graph (`.sd`)

An `.sd` file describes a device as data. It contains no Rust source, executable code or external
assets. The Rust SDK builds and validates the graph, writes the JSON file and can prepare the
supported graph for audio processing.

## Version 1

The top-level object contains `format: "suhina-device"`, `version: 1`, a `device` object, a
`nodes` list and `connections`. Connections determine processing order regardless of node storage
order. The device has a stable id, a display name, a role
(`effect` or `instrument`) and parameter declarations (`id`, `name`, `min`, `max`, `default`,
and a generated UI control). The SDK currently generates sliders from those parameter declarations.
Each node may set `visible: false` to hide it from the read-only graph display; this does not
remove it from audio processing. Nodes are visible by default, including in files written before
this field was added. The detail panel draws visible nodes in signal-flow order.

Version 1 executes effects only. Its supported node kinds are `audio_input`, `gain` and
`audio_output`. A gain node names one declared parameter. Connections join node ids; ports are
stereo audio and implicit. Graphs are linear: exactly one input and output, every node lies on
the path between them, and nodes have at most one input and output.

The reader rejects files larger than 256 KiB, graphs above 128 nodes and 127 connections, and
graphs with more than 128 parameters,
invalid parameter ranges, duplicate ids, missing node or parameter references, unsupported
versions and invalid graph topology. Instrument graphs can be represented and saved, but version 1
does not execute them. A prepared effect is created off the audio thread; parameter updates use
parameter indices and audio processing allocates no memory.

## Example

```json
{
  "format": "suhina-device",
  "version": 1,
  "device": {
    "id": "org.example.gain",
    "name": "Example Gain",
    "role": "effect",
    "parameters": [{ "id": "gain", "name": "Gain", "min": 0.0, "max": 2.0, "default": 1.0, "control": "slider" }]
  },
  "nodes": [
    { "id": "in", "kind": "audio_input", "visible": false },
    { "id": "gain", "kind": "gain", "parameter": "gain", "visible": false },
    { "id": "out", "kind": "audio_output", "visible": false }
  ],
  "connections": [{ "from": "in", "to": "gain" }, { "from": "gain", "to": "out" }]
}
```

The graph can be created with the SDK example and then inserted on a track:

```sh
cargo run -p suhina-device-sdk --example gain -- path/to/gain.sd
```

The browser lists `.sd` files under Effects as `AudioEffect`. Drop one onto a track, run `device.addSd` through MCP,
or pass it to `track.add {sd}`; dropping one into empty space creates an audio track with the effect.
The complete validated graph is
stored in the project with the device, so the original `.sd` file is not needed after insertion.
Parameter values are project state and can be changed with `device.setParam` or the generated
sliders in the detail panel. The `AudioEffect` panel reserves space for each slider's value so
controls stay inside narrow device cards.

Only effects execute. Instrument role can be represented but cannot be inserted or rendered yet.
SDK-authored nodes select from the supported built-in node kinds; arbitrary Rust processors and
custom executable UI code are not loaded into Suhina. Branching, feedback, custom DSP nodes, and
freely arranged graph editing remain future work.
