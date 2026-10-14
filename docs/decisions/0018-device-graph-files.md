---
description: A data-only versioned graph format and Rust SDK for community-authored Suhina effects
read_when: .sd, Suhina device SDK, device graph, community device, graph effect
type: decision
status: current
last_verified: 2026-10-14
covers:
  - crates/device-sdk/Cargo.toml
  - crates/device-sdk/src/lib.rs
  - crates/device-sdk/examples/gain.rs
---

# 0018: Data-only device graphs

- **Status:** accepted
- **Date:** 2026-10-14
- **Task:** #300

## Context

Issue #300 asks for a Rust SDK that can create a shareable `.sd` device file. The file describes
the device and signal flow as data, without embedding Rust source. Version 1 loads linear effect
graphs into tracks and draws their nodes and parameter controls in the detail panel.

## Decision

1. `.sd` is versioned JSON with device metadata, role, parameters, processing nodes and graph
   connections. It contains no executable code.
2. `suhina-device-sdk` is a Rust crate that builds, validates, reads and writes this format and
   prepares supported graphs for processing.
3. Version 1 supports linear stereo effect graphs with audio input, gain and audio output nodes.
   It can represent an instrument role, but does not execute instrument graphs yet.
4. Graph validation and compilation happen off the audio thread. Prepared processing uses fixed
   graph state and does not allocate. Version 1 caps file size, nodes, parameters, and connections.
5. `device.addSd` loads a validated effect onto a track, and the browser accepts `.sd` files as
   effects. The project embeds the graph JSON in the device state, independent of the source file.
   The engine compiles it while building the renderer and swaps graphs through the existing edit
   path.
6. Parameter declarations generate slider controls in the device detail panel. The panel also
   draws the graph's visible nodes as a read-only signal-flow view. SDK authors can hide
   implementation nodes with `visible: false`; hidden nodes still run. Editing node connections in
   the UI is deferred. The panel labels these as `AudioEffect` to distinguish them from plug-ins.

## Consequences

- SDK authors can create a data-only gain effect, its parameter UI, and its visual node flow.
- The graph runs in offline and real-time chains without loading community Rust code.
- Version 1 is intentionally a linear effect graph; branching, feedback, executable custom DSP,
  instruments, asset packaging and graph editing require later decisions and implementation.
