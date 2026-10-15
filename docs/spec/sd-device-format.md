---
description: Version 2 of the data-only Suhina device graph format, SDK, track loading, and generated controls
read_when: .sd, Suhina device, device graph, device SDK, community effect, visual node, device UI
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/device-sdk/Cargo.toml
  - crates/device-sdk/src/lib.rs
  - crates/device-sdk/src/graph.rs
  - crates/device-sdk/src/midi.rs
  - crates/device-sdk/src/runtime.rs
  - crates/device-sdk/examples/gain.rs
  - crates/model/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/session/src/devices.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/chain_tests.rs
  - crates/session/src/devices.rs
  - crates/ui/src/detail_view.rs
  - crates/media/src/info.rs
---

# Suhina device graph (`.sd`)

An `.sd` file describes a device as data. It contains no Rust source, executable code or external
assets. The Rust SDK builds and validates the graph, writes the JSON file and can prepare the
supported graph for audio processing.

## Version 2

The top-level object contains `format: "suhina-device"`, `version: 2`, a `device` object,
`nodes` and `connections`. The reader also accepts version 1 linear input/gain/output graphs;
writing canonical JSON upgrades them to version 2 without changing their sound.
The device has a stable id, name, role and parameters (`id`, `name`, `min`, `max`, `default`,
`control`, `unit`). Controls include `slider` (default), `log_slider` (positive range), `integer`
(integer range/default) and `toggle` (range 0..1, integer default). Unit is display text, at most
128 bytes, default empty. Parameter commands and the processor apply the same clamping/rounding.

Each node has `id`, `kind`, kind-specific configuration and `visible` (default true).
Hidden nodes still process; the UI shows visible nodes in dependency order. It is a read-only
list of nodes, not an editor of the connections. Sequential arrows appear only for linear graphs.

Connections have `from`, `to`, `from_port` (default `out`) and `to_port` (default `in`).
Ports carry stereo sample-rate signals; source ports `left` and `right` broadcast one channel.
Constants and parameters broadcast control values. Each input accepts at most one connection;
branching is allowed, summing requires add/mix. `effect` graphs require one audio input and
output. `instrument` graphs have no audio input, require one audio output, and use timestamped
MIDI events as control inputs. `midi_effect` graphs use one `midi_input` and one `midi_output` in
a single chain; `midi_transpose` changes note-on/off keys and drops keys outside 0..127, preserving
each event's sample offset. Exactly one role-appropriate input/output is required,
and every node must contribute to the output. Missing references, duplicate ids, invalid ports,
missing required inputs and cycles are rejected with node/port diagnostics. Only the `in` port
of sample_delay/delay breaks a cycle; its input is written after all current-frame evaluations.
Delay time is an ordinary dependency, so a cycle through it is rejected.

File size is capped at 2 MiB, graphs at 128 nodes, 640 connections and 128 parameters.
Shared delay history is capped at 2,000,000 stereo frames and validated at 384 kHz, before
allocating history. Accepted graphs fit all supported rates (8..384 kHz). Non-finite signals
become zero and finite results are limited to the f32 range after every node.

Compilation with `compile_at(sample_rate)` prepares all buffers and DSP state off the audio
thread. `process_context` receives the engine transport (tempo, beat, playing); `process`
uses stopped transport defaults. Oscillators and delays continue to advance while stopped.
Parameter updates and processing allocate nothing. `reset` clears history without changing
parameter values. Offline and real-time rendering run this same processor; graph swaps keep
state when the embedded graph is unchanged, including effect tails and oscillator phase.

See the complete [node reference](sdk-nodes.md) and the [authoring guide](../device-sdk.md),
including the machine-readable catalog for AI agents.

## Example

```json
{
  "format": "suhina-device",
  "version": 2,
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

The browser currently lists `.sd` files under Effects as `AudioEffect`. Drop one onto a track or
run `device.addSd` through MCP. Audio effects work on audio or MIDI tracks; MIDI effects and
instruments require a MIDI track. Dropping an `.sd` into empty space or calling `track.add`
with `sd` creates an audio track for an audio effect and a MIDI track for an instrument or MIDI effect.
The complete validated graph is
stored in the project with the device, so the original `.sd` file is not needed after insertion.
Parameter values are project state and can be changed with `device.setParam` or the generated
sliders in the detail panel. The `AudioEffect` panel reserves space for each slider's value so
controls stay inside narrow device cards.

All three roles execute. Instrument graphs expose `midi_key`, `midi_velocity` and `midi_gate`
signals to the existing audio nodes to generate sound. This initial instrument runtime is
monophonic: the latest note controls the graph, and note-off releases its gate. MIDI effects go
before the track instrument; their event buffers are prepared with the chain, so processing does
not allocate on the audio thread.
The `.sd` file contains data only. FFT, sample assets, oversampled nonlinear processing and
freely arranged graph editing are not implemented; accepted graphs need none of these to load.

## Custom parameter controls and smoothing

Parameter `control` accepts `slider`, `log_slider`, `integer`, `toggle`, `knob`, `log_knob`,
`number`, `vertical_slider`, `button`, `choice` and `radio`. Button is a persistent on/off
control. Choice/radio use integer values 0..N-1 and `ui.options` (N nonempty labels, at most
128 labels of 128 bytes). Log controls require a positive minimum. Integer/toggle/button and
choice/radio values stay discrete. All edits use `device.setParam` and persist in the project.

Optional `ui` settings: `tooltip` (up to 1024 bytes), `group` (label prefix, up to 128 bytes),
`step` (nonnegative finite value, default 0), `decimals` (0..8, default 3), and `options`.
Rows use the selected control's height and the panel scrolls when controls exceed its viewport.
Executable UI code and arbitrary pixel layouts are not part of the data format.

`smoothing_ms` defaults to 20 ms (including files without this field), accepts 0..1000 ms,
and ramps continuous parameter changes on the audio thread without allocation. Initial values
are applied immediately before the first frame. Reset settles to the target value. Discrete
controls switch immediately; use a `smooth` signal node when a toggle drives gain. This reduces
parameter step clicks; unstable feedback, discontinuous mode switches and underruns still need
an effect-specific solution. Delay-time ramps can change pitch as the read head moves.

Generate a control gallery with
`cargo run -p suhina-device-sdk --example ui_controls -- target/sdk-examples/ui-controls.sd`.

Example parameter UI declaration:

```json
{"id":"mode","name":"Mode","min":0,"max":2,"default":1,"control":"choice",
 "ui":{"options":["Off","Clean","Warm"],"group":"Tone","tooltip":"Select the processing mode"}}
```

## Visual styling and placement

`ui.size` is an optional [width,height] in logical pixels for the entire parameter cell
(24..4096 wide, 24..4096 high). `ui.position` is optional [x,y] relative to the start of the
parameter area, after the node list (0..8192). Unpositioned cells stack after earlier cells.
Both axes scroll when the authored canvas exceeds the viewport. Explicit cells may overlap;
parameters later in the array draw later. Layout is stored as data in the embedded graph.

Optional RGB triplets `ui.accent`, `ui.background`, `ui.panel_color` and `ui.text_color` style controls, frames
and text. Omitted colours follow the Suhina theme. `ui.font_size` is 8..48 (default 12),
`ui.monospace` selects the font family, and `ui.corner_radius` is 0..128 (default 4).
`ui.shape` is rounded (default), square or pill; knobs use their circular body by default
and a square body for square. `ui.label_position` is left (default), top or hidden;
`ui.show_value` defaults to true. Knob size follows the available control area.

These fields are optional and old files retain automatic placement. Authors must leave enough
cell space for labels and controls. Untrusted geometry is checked before UI rendering;
there is no executable UI code, external font loading or arbitrary drawing script.

`background` colours widget bodies; `panel_color` colours the entire cell independently.

For a compact knob use `size: [40,40]`, `label_position: "hidden"`, `show_value: false`.

## Authored canvases and embedded images

Optional `device.ui` adds a canvas with backgrounds, embedded PNG/JPEG images, drawing expressions
and custom parameter controls. Canvas `size` defaults to `[640,240]`; optional `viewport_size`
(default null, automatic) independently sets the preferred scrollable body dimensions. See the [custom panel contract](sdk-custom-ui.md) for every field,
gesture and resource limit. Graphics are evaluated on the UI thread and are stored in the same
`.sd` as the audio graph. They do not change its DSP execution contract.
