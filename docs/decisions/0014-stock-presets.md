---
description: Presets that ship with Suhina for its built-in effects and instruments - where they live, their licence (CC0), what a contribution needs, and the proposed file format
read_when: preset, stock preset, factory preset, preset contribution, CC0, presets folder, preset format, preset licence, DX7 bank
type: decision
status: current
last_verified: 2026-10-13
covers:
  - presets/**
  - .github/PULL_REQUEST_TEMPLATE/preset.md
  - .github/ISSUE_TEMPLATE/5-preset.yml
  - crates/device-registry/src/lib.rs
---

# 0014: Stock presets

- **Status:** accepted (location, licence, contribution rules); the file format is proposed and
  is settled when the first preset loader for a built-in device is built
- **Date:** 2026-10-13

## Context

Built-in effects and instruments ([ADR 0004](0004-built-in-devices.md)) need starting points:
presets that ship with Suhina. Contributors will want to add them. The only preset Suhina reads
today is a DX7 voice for the FM synth (`device-registry::preset`); a preset format of Suhina's
own is listed as not decided in the [browser spec](../spec/browser.md).

Presets are data, not code: a device id and parameter values, a name and a few tags. Whether a
contributor made a preset themselves cannot be checked, any more than for code. A plain list of
parameter values has thin protection, if any; what can carry rights is a whole bank, its names
and descriptions, and the files themselves.

## Decision

1. **In this repository, under `presets/<device id>/`**, one file per preset, so a device and
   its presets can change in one pull request and are tested by the same checks. They can move
   to a repository of their own later without changing the format.
2. **Licence: CC0-1.0.** Everything under `presets/` is dedicated to the public domain, so music
   and presets people make from them carry no conditions. GPL fits data like this badly.
   `presets/README.md` says so and every pull request that adds a preset agrees to it.
3. **What a contribution needs:**
   - the contributor's sign-off ([DCO](../../CONTRIBUTING.md#developer-certificate-of-origin)),
     which states they have the right to submit it under CC0;
   - names, tags and descriptions that do not name or imitate commercial products or their
     presets ("warm bass", not "<product> bass");
   - no banks or files from commercial products, for example the factory voice banks of a
     hardware synthesizer, even when Suhina can read their format;
   - parameter values inside each parameter's range, with every parameter given.

   There is no clean-room review for presets: making a sound is not reimplementing a product. If
   a report says a preset infringes, [`IP.md`](../../IP.md) applies like for code.
4. **Tests:** every preset file is read and checked against the device's parameter descriptions
   (known parameter ids, values in range), and loading it with `device.add {preset}` gives the
   same parameter values. A render of a short phrase through each preset is added when the device
   has golden renders.
5. **Proposed file format** (`.json`, at most 64 KiB like any preset file the browser reads):

   ```json
   {
     "format": "suhina-preset",
     "version": 1,
     "device": "compressor",
     "name": "Gentle bus glue",
     "author": "github-handle",
     "tags": ["bus", "gentle"],
     "params": { "threshold": -18.0, "ratio": 2.0, "attack": 0.01, "release": 0.3 }
   }
   ```

   `params` holds plain values (seconds, decibels…) by the parameter's stable id (ADR 0004), the
   same way a project stores a device. A reader ignores unknown keys and fills a missing
   parameter with its default, so a preset keeps working when a device gains parameters.

## Consequences

- `.github/PULL_REQUEST_TEMPLATE/preset.md` and the *Preset contribution* issue form ask for the
  points in 3; [`CONTRIBUTING.md`](../../CONTRIBUTING.md) links them.
- The browser lists presets from `presets/` next to the user's own once the loader exists.

## Not decided yet

- The loader in `device-registry` and the browser entries for stock presets.
- Saving the user's own presets (same format, in the settings folder).
- CLAP preset discovery and VST3 presets for plug-ins.
