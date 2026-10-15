---
description: The interface of built-in effects and instruments (Device trait, parameters, real-time rules, contract tests) and the dsp crate
read_when: device, built-in effect or instrument, effect, instrument, dsp, filter, parameter, automation, utility, device-api, conformance
type: decision
status: current
last_verified: 2026-10-15
covers:
  - crates/dsp/**
  - crates/device-api/**
  - devices/**
  - crates/device-registry/**
  - crates/engine/src/chain.rs
  - crates/engine/src/buses.rs
  - crates/session/src/devices.rs
---

# 0004: Built-in effects and instruments

- **Status:** accepted
- **Date:** 2026-10-11
- **Task:** LOOP-33

## Context

Suhina ships its own instruments and effects ([`ROADMAP.md`](../../ROADMAP.md) › Built-in
effects and instruments): first a utility and an auto-pan, then a DJ EQ, distortion, dynamics, delays, reverbs
and synths. Projects imported from other DAWs refer to those DAWs' devices, which come in as
placeholders today (`DeviceKind::Foreign` in the model) and should later map onto ours.

Built-in effects and instruments run inside the engine (AGENTS.md rule 5 covers third-party plug-ins only), so
they are bound by the same rules as the engine: no allocation, locking or I/O on the audio
thread, and no panics. They also have to work from every front end: each parameter is a command
(rule 6) and can be automated.

At the time of writing, the engine mixed everything into one stereo bus and had no per-track
chains, and making edits reach a playing engine (LOOP-26, LOOP-32) was in progress. Devices were
not to wait for that, and the device code was not to change when it landed; chains landed with
decision 9.

## Decision

1. **Three kinds of crate.** `crates/dsp` (`suhina-dsp`) holds building blocks: gain
   conversions, parameter smoothing, filters, crossovers, LFOs, shapers, DC filtering and
   oversampling; later oscillators, envelopes and delay lines. `crates/device-api` (`suhina-device-api`) holds the interface. Each
   device is its own crate under `devices/` (`suhina-device-<name>`), depending on those two
   only. None of them depends on the model, the engine or each other, so a device can be tested
   alone and later built as a CLAP plug-in or moved to its own repository.
2. **A device describes itself statically.** `DeviceInfo` gives a stable id (the id stored in the
   model as `DeviceKind::Builtin`), a display name and the parameters in a fixed order. Each
   `ParamInfo` has a stable id, a name, a kind (continuous with a linear or logarithmic scale,
   toggle, or choice), a range, a default and a unit.
3. **Parameter values are plain values in their unit** (decibels, hertz, a fraction for a
   percentage, an option index for a choice), not 0..1. A stored value keeps its meaning when a
   range changes, and import mappings convert units directly. `ParamInfo` converts to and from
   the 0..1 travel of a knob or an automation lane, and `clamp` turns any number, NaN included,
   into a valid value.
4. **The `Device` trait.** Construction, `prepare(sample_rate)` and drop run off the audio thread
   and may allocate. `reset`, `set_param(index, value)`, `param(index)` and
   `process(context, buffer)` run on it and must be real-time safe and panic-free whatever they
   are given. `process` works in place on interleaved stereo, the engine's format, and gets a
   `ProcessContext` (tempo, song position in beats, whether the transport runs) for devices that
   follow the tempo. `latency()` reports added delay for compensation.
5. **Automation: the host splits, the device smooths.** The host addresses parameters by index
   and splits a block where an automation point falls, so a change lands on its frame; the
   device ramps a change that would click (20 ms for the utility). The output must not depend on
   how a stream is divided into blocks.
6. **Every device passes the same contract tests.** `device_api::conformance::check` runs a
   device through hostile parameter values and input (NaN, infinities, extremes), several block
   sizes with parameter changes, `reset` against a fresh device, and every sample rate from
   8 kHz to 384 kHz. Each device crate calls it from a test, next to its own tests of what it
   sounds like.
7. **Order of work per device** stays as the roadmap says: observed behaviour written down, our
   own design, DSP, import mapping, tests. The import mapping lives in the format crate
   (`crates/als`), never in the device.
8. **Instruments take notes as events, between blocks** (added 2026-10-12 with the first one,
   the FM synth, LOOP-16). `DeviceInfo::role` says whether a device is an `Effect` or an
   `Instrument`; an instrument writes its output over the buffer. `Device::event` hands it a
   `NoteEvent` (key on and off with velocity 0..1, a MIDI controller 0..1, pitch bend -1..1,
   pressure, all notes off) at the frame where the next `process` starts: the host splits a
   block where an event falls, exactly as for automation, so one mechanism times both and the
   trait needs no event list. Values are plain and in range from a well-behaved host, but a
   device accepts anything without misbehaving. Effects ignore events. Whole-number parameters
   (a level from 0 to 99, an algorithm number) get their own kind, `ParamKind::Integer`, so they
   never take a fraction. The contract tests add, for instruments, a fixed performance (notes
   that start inside blocks, the sustain pedal, bend, the wheel, pressure) in every block size,
   `reset` while notes sound, hostile events (keys over 127, values that are not numbers, far
   more notes than voices), silence after all notes are released, and every sample rate.

9. **The engine runs device chains on tracks** (added 2026-10-12, LOOP-35).
   `crates/device-registry` makes a device from its `DeviceKind::Builtin` id. When the renderer's
   graph is built, a track whose channel holds enabled built-in effects and instruments gets a chain: the
   devices made, prepared, given the project's parameter values (`Device::params`, by
   parameter id; missing ones at their defaults) and reset. Everything the track plays in a
   block (sine voices, audio clips, stretched and launched clips) goes into the chain's buffer
   at a centred unity gain instead of the output; the devices run over it in order, and then
   the track's pan and volume (and those of the channels it feeds) apply, so the fader is
   after the devices. A track whose cue is on also has a chain (a passthrough one without
   devices), which adds its output to the cue bus too ([cue spec](../spec/cue-output.md)).
   A muted track, or one a solo leaves out, keeps its chain at a gain of
   zero, so its devices (and plug-ins, which would otherwise be let go of) keep running and
   sound again at once. Every device gets the block's own transport (tempo, beat, playing),
   also when its chain runs in the stages plug-ins share (ADR 0006, decision 4). When the
   first device is an instrument, the track's notes (arrangement
   and launched) go to it as note events at their frames, the chain running in pieces split at
   each event, and note-offs follow from each note's end; stopping the transport or a column
   releases them. A track without devices is mixed straight into the output exactly as
   before. Unknown ids, plug-ins that are not installed and other DAWs' devices pass the sound
   through; installed plug-ins run in the same chain ([ADR 0006](0006-plug-in-hosting.md)), so
   the devices of a chain run one after another over a whole block, each split at the events.
   Chains
   hold at most 1024 frames, so the renderer splits longer blocks. A graph swap (ADR 0005)
   hands a chain's running devices to the new graph when the same devices sit on the same
   track, so held notes and tails carry on through an edit and the new parameter values
   apply; otherwise the new chain starts fresh and the old devices are freed with the old
   graph, off the audio thread. A render with chains is 2 s longer, for releases and tails.

10. **A group with devices is a bus** (added 2026-10-12, with Stems files, whose mastering
   compressor and limiter must act on the sum of the stems). When a group track's channel holds
   devices, the tracks routed into it (at any depth) no longer reach the output on their own:
   each gets a chain, one without devices when it has none, whose output goes into the group's
   chain buffer at the track's pan and the volumes from its channel up to the group; the
   group's devices run over the sum, and the volumes from the group to the master apply after
   them (a group's pan does not apply, as before). Chains are ordered so a bus comes after
   every chain that feeds it, and a block runs them in that order. Latency compensation follows
   every path: a bus's input waits for the slowest chain feeding it, and the output for the
   slowest path, so a track with a lookahead device inside a group with another still starts
   with the rest (`crates/engine/src/buses.rs`). A device reports its delay with `latency()`
   (the overdrive and vinyl distortion, 31 frames; the limiter, its 1.5 ms lookahead), and every
   other track is delayed to match ([ADR 0006](0006-plug-in-hosting.md) › Consequences). A group without devices stays a gain on the
   tracks routed through it, with no buffer of its own.

## Not decided yet

- **Automation lanes** for device parameters (LOOP-39).
- **Chains on return and master channels, and sends into chains:** how they run and how latency
  compensation (built for track and group chains, 10 above) extends to them.
- **Mono, multichannel and side-chain inputs.** The trait is stereo in, stereo out.
  Side-chained dynamics extend it when the first one is built.
- **Per-note expression.** `NoteEvent` has no channel and no per-note controls yet (MIDI
  Polyphonic Expression, per-note pitch); they come with the first instrument that uses them.

## Consequences

- Devices were written and tested in parallel with the engine work, and plugged into the
  engine without changes once it had chains.
- The first device, `utility`, proves the shape: thirteen parameters, all smoothed, bit-exact
  passthrough at the defaults.
- Devices are edited through commands: `devices.list`, `track.devices`, `device.add` (an
  instrument always first, one per track, and only on a track that plays notes),
  `device.remove`, `device.setEnabled` and `device.setParam` (values stored limited to the
  parameter's range). `device.add {preset, voice?}` adds a device with a preset's values:
  `device-registry::preset` reads DX7 voice data for the FM synth and gives every parameter's
  value, which the project stores like any other (stock presets for the other devices:
  [ADR 0014](0014-stock-presets.md), their loader is not built yet).
- Parameter ids and device ids are stored in projects: renaming one is a breaking change, like a
  command id (ADR 0002).
- An instrument that works in fixed internal blocks (the FM synth renders 64 frames at a time)
  applies an event at its next internal block boundary on a grid counted from `reset`, which
  keeps the output independent of the host's block sizes at the cost of up to one internal
  block of timing.
