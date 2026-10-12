---
description: How third-party plug-ins are hosted out of process (process model, IPC, deadline, crashes), the unsafe exception, formats and the VST2 policy
read_when: plug-in, plugin host, CLAP, VST3, VST2, sandbox, out of process, IPC, shared memory, unsafe, plug-in crash, scanning, plug-in editor
type: decision
status: current
last_verified: 2026-10-12
covers:
  - crates/ipc/**
  - crates/plugins/**
  - crates/plugin-host/**
---

# 0006: Plug-in hosting

- **Status:** accepted
- **Date:** 2026-10-11
- **Issue:** [#30](https://github.com/juhamikael/Loopwerk/issues/30)

## Context

AGENTS.md rule 5: third-party plug-ins never run in the Loopwerk process. A plug-in that crashes
or hangs must not stop the engine or lose the project; the host reports it, keeps the project
intact and can restart it. Projects imported from other DAWs already keep their plug-ins' ids
and state (`DeviceKind::Clap`, `Vst3`, `Vst2` with `Device::state`); nothing runs them yet.

**What other hosts do** (sources at the end):

- Bitwig Studio runs plug-ins in sandbox processes: all together (its default), per vendor, per
  plug-in, or per instance, with a "Reload Plug-in" button when one crashes. REAPER bridges
  plug-ins into one shared or one dedicated process on request; its default is in process.
- Carla's bridges and yabridge keep audio in shared memory and synchronise once per block.
  yabridge uses one process per plug-in by default and groups for plug-ins whose instances talk
  to each other.
- Ardour does not sandbox: by its estimate each crossing costs two context switches, about 30 µs,
  which adds up when every plug-in crosses on its own. Apple measured about 40 µs per render
  cycle for out-of-process Audio Units. Grouping plug-ins so that one crossing serves several of
  them is what keeps the cost down.

**Formats and licences** (checked 2026-10-11):

- CLAP is MIT. Rust: `clap-sys` and `clack` (MIT/Apache-2.0).
- The VST3 SDK is MIT since version 3.8 (October 2025); GPLv3 and Steinberg's own licence no
  longer apply to it. Rust: `vst3` (MIT/Apache-2.0), bindings generated from the SDK.
- VST2: Steinberg stopped signing VST2 agreements in 2018 and has said new VST2 hosts cannot be
  licensed. Its SDK licence is not GPL-compatible. Free headers exist (VeSTige, GPLv2+; FST,
  GPLv3+), but VeSTige's origin is known only second-hand and was the target of a DMCA notice in
  2018, and FST takes names from JUCE sources that shipped Steinberg's headers, which our
  clean-room rules do not allow.
- VST3 has a documented way to replace a VST2 plug-in: the VST3 version declares the VST2 class
  it replaces (`moduleinfo.json` `Compatibility`, SDK 3.7.5 and later, or
  `IPluginCompatibility`, or the documented class-id convention built from the VST2 id and
  name), and accepts the VST2 state (fxb/fxp) in `setState`. Cubase, Nuendo, REAPER and Studio One
  use it.

**`unsafe`:** loading a plug-in is calling into a foreign C ABI, and memory shared between
processes can change under Rust's feet; neither is possible without `unsafe`, which the
workspace forbids.

## Decision

1. **The engine never loads a plug-in.** A separate executable, `loopwerk-plugin-host`, loads
   plug-ins and runs them; the engine talks to it only through the `ipc` crate.
2. **Process model.** By default one host process per plug-in binary (every instance of one
   plug-in together), so plug-ins whose instances share state still work and one crash takes
   down one product, not all. Per plug-in the user can choose *its own process per instance* or
   *one process per vendor*. Where consecutive plug-ins of a chain live in the same process, they
   run in one round trip per block.
3. **The boundary.**
   - **Shared memory**, one region per host process: a block header (frame count, transport,
     a sequence number), the audio buffers sized for the largest block, and lock-free rings of
     note and parameter events with sample offsets, so automation stays sample-accurate.
     *Built:* each block carries up to 1024 events (note on and off, MIDI controller, pitch
     bend) as fixed 16-byte records with their frame (`crates/ipc/src/events.rs`, region
     version 2); the host reads them into a list made beforehand, so neither side allocates.
     A VST3 host hands notes to `process` as an `IEventList` on event bus 0, and controllers
     and pitch bend as parameter changes at their frame through the plug-in's `IMidiMapping`
     (asked for the first MIDI channel only: Serum 2 crashes when asked about the second); a
     CLAP host sends note events to note port 0 and controllers and bend as MIDI messages, and
     gives a plug-in without audio inputs (an instrument) no input port.
   - **Messages** over a local socket or pipe, with a versioned protocol: load and unload,
     state save and load, latency changes, editor open and close, ping. *Built:* parameters
     (protocol version 3): `get_params` lists each with its id, name, group, range, default,
     current value, the plug-in's own text for it, steps and flags; `set_param` sets one in its
     own range (a VST3 plug-in's normalized, a CLAP plug-in's plain), on a VST3 controller at
     once and on the processor (VST3 parameter changes, CLAP parameter events) at the start of
     the next block. The engine side keeps values set since the state was last read and sets
     them again after a restart.
4. **Block sync and deadline.** The engine signals the host, the host processes and signals
   back. Primitives: a futex in the shared memory on Linux; duplicated auto-reset events on
   Windows; semaphores on macOS, prototyped first because they are the least certain. The host's
   audio thread runs at the engine's real-time priority (MMCSS "Pro Audio" on Windows; the
   engine's audio workgroup on macOS). The engine waits at most the rest of the block's time.
   On a miss it outputs the plug-in's input (an effect) or silence (an instrument) for that block
   and counts the miss. Repeated misses, a dead process or an unanswered ping mark the plug-in
   *crashed* or *hung*; the engine kills the process and offers a restart. Offline renders wait
   without a deadline (with a long timeout), so they run as fast as the plug-in allows.
5. **State survives crashes.** The host pulls each plug-in's state on save and periodically;
   the project keeps it in `Device::state`. A restart loads the last state and keeps the latency
   the chain was compensated for.
6. **`unsafe` only in two crates.** `crates/ipc` (shared memory and block sync, linked into the
   engine) and `crates/plugin-host` (the plug-in host process) may use `unsafe`; every other
   crate stays `forbid`. Both offer safe APIs, keep `unsafe` blocks small, explain each one in a
   `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` denied there) and test them. `ipc`
   runs inside the engine process, so it stays small.
7. **Formats.** CLAP first, then VST3, on the same boundary. CLAP goes through `clack-host`:
   the instance stays on the host's main thread (with the plug-in's main-thread callbacks), the
   started audio processor on its audio thread. A plug-in is named `clap:<file>` or
   `clap:<file>#<id>`; the only `unsafe` the host needs for it is opening the file. A CLAP gain
   built into the host (`test:clap`, made with `clack-plugin`) lets the tests run the whole path
   without a plug-in installed; `LOOPWERK_TEST_CLAP` runs them against a real file too.
   VST3 goes through the `vst3` bindings to the SDK: the component and controller on the main
   thread (connected through their connection points, the controller given the component's
   state), `setProcessing` and `process` on the audio thread, stereo on the main buses and
   silent buffers for any other bus. A plug-in is named `vst3:<bundle or file>[#<class id>]`;
   the state handed to the engine is the component's. `test:vst3`, a gain with a separate
   controller built into the host, plays the part of `test:clap`. **Class ids** are written as
   the SDK writes them (`moduleinfo.json`, the model): on Windows the first three fields of the
   COM GUID layout are turned back into numbers, so the text matches across scanning, the
   module's own description and imported projects.
8. **VST2: through VST3, no VST2 host for now.** A project that names a VST2 plug-in is opened
   with the installed VST3 that declares it replaces it, and the saved VST2 state is handed to
   it. Without such a VST3 the device stays in the project as a *missing plug-in* with its state
   intact, and the user can relink it by hand to a VST3 or CLAP version. Hosting VST2 itself is
   a separate decision (below).

   *Finding the replacement* (`crates/plugins/src/matching.rs`, `plugins.check`). The catalog is
   searched in order of how sure the answer is: the same id; an installed VST3 that declares it
   replaces the plug-in (`Compatibility` in `moduleinfo.json`, or the module's
   `Plugin Compatibility Class`, which the scanner asks through `IPluginCompatibility`); a VST3
   whose class id is derived from the VST2 id by the SDK's convention (`VST`, the 4-byte id, the
   first nine bytes of the name in lower case; the name part is not compared, as a project keeps
   the name its host showed); the same name once build words (`x64`, `64bit`) and a leading
   vendor name are set aside; and last the nearest newer version of the same name (`Serum` →
   `Serum 2`, `Pro-Q 2` → `Pro-Q 3`). Only the first three promise that the state carries over;
   for the last two the state is handed over and the plug-in decides, and the result says
   which (`keeps_state`). Vendors declare their own older builds rather than older products:
   the Pro-Q 3 module declares the VST2 builds of Pro-Q 3, not Pro-Q 2, which is why the name
   steps exist.

   *Handing over the state.* The importer keeps a VST2 plug-in's state as the standard VST2
   bank (`CcnK`, the format the SDK's `vst2persistence` reads), and the host hands it to the
   VST3 component's `setState` and mirrors it to the controller. A VST3 project's controller
   state goes with the component's (`set_state` carries both, protocol version 2) and is
   restored after a restart. A plug-in that refuses the state runs with its defaults and the
   caller is told (`offline::render_with_state`, `loopwerk-cli plugin run-device`). Measured on
   a real project: Valhalla VintageVerb and PSP VintageWarmer read their VST2 chunk banks
   (the output changes with the saved settings); OTT takes a parameter bank (`FxBk`) in
   `setState` without applying it. So the host also sets a parameter bank's current program as
   parameters, value `i` to parameter id `i`, the numbering a VST3 version keeps from its VST2
   one, but only when the plug-in has every id from 0 to the last value's: on the controller
   (`setParamNormalized`) and on the processor through the next block's parameter changes
   (`crates/plugin-host/src/vst3_params.rs`). With that OTT plays with the project's settings.
   Pro-Q 3 refuses Pro-Q 2's state and runs with its defaults. Sylenth1 (an instrument, VST2
   to its VST3 by the id convention) plays a project's notes with its saved settings. Serum 2
   refuses Serum 1's state in every form tried: the bank, a single program (`FPCh`, an `.fxp`
   file's contents) and the bare chunk; it opens Serum 1 presets only from its own browser, so
   a project's Serum plays with Serum 2's defaults until a way is found.

   In a real project with 14 different VST2 plug-ins, 4 were found by the id
   convention, 1 by name and 3 as newer versions; the other 5 exist on that machine only as
   VST2.
9. **Scanning** runs in its own short-lived process with a timeout per plug-in, a cache keyed by
   path, modification time and size, and a block list of plug-ins that crashed or hung. CLAP's
   factory and VST3's `moduleinfo.json` are read without instantiating plug-ins where possible.
   It looks in the system's standard folders (and, for CLAP, the folders in `CLAP_PATH`) plus
   folders the user adds, so no setting is needed to find installed plug-ins. Implemented in
   `loopwerk_plugins::scan` and `scan_pool` for CLAP and VST3:
   - A VST3 bundle with a `moduleinfo.json` (JSON5) is read from it without loading anything.
   - Everything else goes to a few long-lived `loopwerk-plugin-host --scan-worker` processes
     (half the processor's threads, at most 4), each reading file after file. A worker that
     crashes or hangs on a file is ended, the file is marked failed with the reason, and a new
     worker takes the rest. Shells (files with many plug-ins, such as Waves' WaveShell) go first
     and get 180 s, other files 30 s.
   - Measured on a Windows machine with 443 plug-in files: a process per file took 11.5 min and
     timed the shells out; this takes 86 s and finds every plug-in (733). Starting a process
     cost about a second there (antivirus checks each start), more than most plug-ins take to
     load.
   - A file that failed keeps its reason and is not tried again until it changes.
   The `plugins.*` commands run scans and choose the added folders (ADR 0002 amendment); the
   folders and the catalog are kept as JSON in Loopwerk's settings folder.
10. **Editors** open from the host process: embedded in a Loopwerk window on X11 and Windows,
    floating on Wayland and macOS, which have no public way to embed another process's window.
    *Built on Windows:* the host opens a top-level window of its own on its main thread
    (`crates/plugin-host/src/window.rs`, Win32) and attaches the plug-in's view to it (VST3
    `IPlugView` with an `IPlugFrame` for size requests; CLAP's `gui` extension, embedded), pumps
    its messages in the main loop between control messages, and tells the engine unasked when
    the user closes it (`editor_closed`, protocol version 4). Until Loopwerk has a window of its
    own, the editor's window is a top-level one rather than embedded in Loopwerk's. Checked with
    Valhalla VintageVerb and Pro-Q 3 (VST3); no CLAP plug-in with an editor was at hand. X11 and
    the floating cases come later.
11. **Other architectures later, same protocol:** a 32-bit Windows host, an x86-64 host on Apple
    silicon, Windows plug-ins on Linux through Wine.
12. **Windows plug-ins on Linux: yabridge first.** Linux has native CLAP and VST3 plug-ins, which
    load like any other. Windows-only plug-ins reach Linux through yabridge (GPL-3.0), which
    wraps each one as a Linux CLAP or VST3 file; Loopwerk loads those like native plug-ins once
    CLAP and VST3 hosting work, with nothing of its own to build. It costs a second process hop
    (our host, then yabridge's Wine process). Windows plug-ins that exist only as VST2 do not
    work this way (decision 8).

## Not decided yet

- **Hosting VST2 directly.** It would need our own clean-room description of the VST2 ABI
  (observed from plug-ins and hosts we may use, written in `docs/spec/`), an optional crate kept
  apart from the rest, no use of the VST trademark, and legal advice before release. Decided
  when we know which plug-ins people still use only as VST2.
- **Our own Wine host** for Windows plug-ins on Linux (`loopwerk-plugin-host` built for Wine,
  speaking the same protocol, so one hop instead of two and one place for crashes, scanning and
  editors). Decided after native CLAP and VST3 work and the cost of the yabridge path has been
  measured; yabridge's code (GPL-3.0) can be read for how it solves the same problems.
- macOS block sync and audio workgroup details, after the prototype.
- Audio Units on macOS, LV2 on Linux, ARA (LOOP-15).

## Consequences

- Every host process costs a round trip per block, tens of microseconds; at small blocks with
  many plug-in products this is noticeable. Grouping by product and running chains in one round
  trip keep it bounded; *in process* is not offered.
- A crash costs the engine one block of the plug-in's output, not the session or the project.
- Measured with `ipc` on Windows (test `round_trips_with_another_process`, 128 frames, the peer
  at normal priority): blocks back to back take 0.7 µs per round trip, because the peer is still
  spinning; with 2.5 ms between blocks, as in playback, 19 µs on average and 0.8 ms at worst,
  the worst case being why the host's audio thread needs real-time priority.
- Work goes in this order: this ADR; `ipc` with a host that crashes and hangs on purpose; CLAP;
  a CLI command that runs a WAV through a plug-in; VST3; finding the replacements for a
  project's VST2 plug-ins (done), then loading them with their state; plug-ins in the
  engine's track chain together with built-in devices (LOOP-35); editors (Windows done).

## Sources

- Bitwig plug-in hosting modes: https://www.bitwig.com/userguide/latest/vst_plug-in_handling_and_options
- REAPER bridging: https://reaper.blog/2012/02/run-plugin-as-dedicated-process/
- Carla bridge protocol and semaphores: https://raw.githubusercontent.com/falkTX/Carla/main/source/utils/CarlaBridgeDefines.hpp, https://raw.githubusercontent.com/falkTX/Carla/main/source/utils/CarlaSemUtils.hpp
- yabridge architecture: https://github.com/robbert-vdh/yabridge/blob/master/docs/architecture.md
- Ardour on in-process plug-ins: https://ardour.org/plugins-in-process.html
- Apple, out-of-process Audio Units (WWDC 2015, session 508): https://nonstrict.eu/wwdcindex/wwdc2015/508/
- CLAP: https://github.com/free-audio/clap; clack: https://github.com/prokopyl/clack
- VST3 SDK under MIT: https://github.com/steinbergmedia/vst3sdk; vst3 crate: https://github.com/coupler-rs/vst3-rs
- VST2 licensing for hosts: https://forums.steinberg.net/t/can-i-create-vst-2-support-for-host-application-vst-3-license/202012
- DMCA notice against VeSTige (2018): https://lists.linuxaudio.org/archives/linux-audio-dev/2018-June/037132.html
- FST: https://sources.debian.org/src/fst/0.177.0-1/README.md
- VST3 `moduleinfo.json` and compatibility: https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/VST+Module+Architecture/ModuleInfo-JSON.html
- VST2 compatibility FAQ: https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Compatibility+with+VST+2.x+or+VST+1.html
- VST2 state in VST3 (`vst2persistence.h`): https://github.com/steinbergmedia/vst3_public_sdk/blob/master/source/vst/utility/vst2persistence.h
- Hosts that migrate VST2 to VST3: https://u-he.com/support/faq/troubleshooting/vst3-migration/
- Cross-process window embedding on Windows: https://devblogs.microsoft.com/oldnewthing/20130412-00/?p=4683
