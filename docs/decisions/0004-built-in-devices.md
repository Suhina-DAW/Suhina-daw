# 0004: Built-in devices

- **Status:** accepted
- **Date:** 2026-10-11
- **Task:** LOOP-33

## Context

Loopwerk ships its own instruments and effects ([`ROADMAP.md`](../../ROADMAP.md) › Built-in
devices): first a utility and an auto-pan, then a DJ EQ, distortion, dynamics, delays, reverbs
and synths. Projects imported from other DAWs refer to those DAWs' devices, which come in as
placeholders today (`DeviceKind::Foreign` in the model) and should later map onto ours.

Built-in devices run inside the engine (AGENTS.md rule 5 covers third-party plug-ins only), so
they are bound by the same rules as the engine: no allocation, locking or I/O on the audio
thread, and no panics. They also have to work from every front end: each parameter is a command
(rule 6) and can be automated.

The engine still mixes everything into one stereo bus and has no per-track chains; making edits
reach a playing engine (LOOP-26, LOOP-32) is in progress. Devices should not wait for that, and
the device code should not have to change when it lands.

## Decision

1. **Three kinds of crate.** `crates/dsp` (`loopwerk-dsp`) holds building blocks: gain
   conversions, parameter smoothing, filters, crossovers, later oscillators, envelopes, delay
   lines and oversampling. `crates/device-api` (`loopwerk-device-api`) holds the interface. Each
   device is its own crate under `devices/` (`loopwerk-device-<name>`), depending on those two
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

## Not decided yet

- **Where parameter values live in the project.** The model's `Device` has no parameter list
  yet; built-in devices will need one (values by parameter id). It is added together with the
  engine side, below.
- **The engine side:** per-track buffers, device chains in the renderer, a registry from
  `DeviceKind::Builtin` ids to constructors, parameter commands in `session`, and automation
  lanes. This needs the renderer swap from LOOP-26/LOOP-32 and gets its own task.
- **Mono, multichannel and side-chain inputs.** The trait is stereo in, stereo out. Instruments
  (which take notes) and side-chained dynamics extend it when the first one is built.

## Consequences

- Devices can be written and tested now, in parallel with the engine work, and plug into the
  engine without changes once it has chains.
- The first device, `utility`, proves the shape: twelve parameters, all smoothed, bit-exact
  passthrough at the defaults.
- Parameter ids and device ids are stored in projects: renaming one is a breaking change, like a
  command id (ADR 0002).
