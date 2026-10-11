# Roadmap

Loopwerk is pre-alpha. Milestones are in order; each one builds on the previous.

| | Milestone | Status |
|---|---|---|
| M0 | **Foundation:** workspace, GPL-3.0-or-later, clean-room rules, DAW-neutral model | done |
| M1 | **First sound:** offline renderer and the `.als` reader | done |
| M2 | **Real-time playback:** audio output, `loopwerk-cli play` | done (verified on Windows) |
| M3 | **Commands and agents:** command registry with undo; CLI and MCP server on the same commands | in progress |
| M4 | **Clip launching:** launch quantisation, scenes, stop buttons, follow actions | in progress |
| M5 | **Audio playback:** imported audio clips with gain, transpose, warping and time-stretching | in progress |
| M6 | **Plug-ins:** CLAP hosting, sandboxed out of process (AGENTS.md rule 5); VST3 after | |
| M7 | **More formats:** DAWproject import and export | |
| M8 | **UI:** Session and Arrangement views on top of the commands; familiar workflow, Loopwerk's own look; installers (AppImage, deb, Flatpak, MSI) and signed macOS and Windows builds | |

## Built-in devices

Instruments and effects that ship with Loopwerk. They run inside the engine (rule 5 covers
third-party plug-ins only), so they follow the real-time and never-crash rules like the engine
itself. Written in Rust; DSP from GPL-compatible open-source projects may be ported (rule 9) with
its copyright notices kept and credited in [`ATTRIBUTION.md`](ATTRIBUTION.md). An ADR comes before
the first device.

- **Crates, in this repository:** `dsp` (filters, oscillators, envelopes, delay lines,
  convolution, oversampling; no engine or model dependency), `device-api` (the device trait,
  parameter descriptions, state) and one crate per device under `devices/`. Every parameter is a
  command and can be automated, from the UI, CLI and MCP alike. Because devices do not depend on
  the engine, they can later be split into their own repository or built as CLAP plug-ins.
- **Importing devices from Live Sets, in three levels:** a placeholder that keeps the parameters;
  then an approximate mapping to an open-source plug-in over CLAP; then a Loopwerk device for the
  most used ones. After import, the user sees which devices came in exactly, approximately or as
  placeholders. No content from Live's library (presets, wavetables, samples, impulse responses).
- **Order:** utility (gain, width, mono, phase) and auto-pan first, to prove the device system;
  then three small devices users have asked for: a three-band DJ EQ with kills, vinyl distortion
  (tracing, pinch, crackle) and a pedal-style overdrive; then bus compressor, multiband dynamics with upward and downward compression, delays,
  algorithmic and convolution reverb, multi-stage saturation; then an FM synth (DSP core ported
  from Dexed/MSFA) and a wavetable synth (from Vital, under its own name).

### How each device is built

Every Loopwerk device follows the same path:

1. **Learn what the device does, not how it is coded.** From the vendor's published manual, from
   listening, and from Live Sets we author ourselves, where we change one parameter at a time and
   record in `docs/spec/` how it is stored (name, range, unit, curve). No binaries are opened.
2. **Design our own device** that covers those features, with its own name, parameter layout
   and look. It does not have to match one to one; it has to be able to take the imported
   parameters.
3. **Build the DSP** in `dsp` and the device crate, porting from open-source code where it helps.
   The licence is checked before the first line is ported, and every ported file keeps its
   notices.
4. **Write the import mapping** in `crates/als`: each stored parameter maps to ours, with a
   conversion where the units or curves differ. Parameters with no counterpart are kept and
   listed, so a later version can use them.
5. **Test** with golden renders of the device alone and of imported sets, compared against the
   bounces we made in Live when authoring the sets. The goal is close, not identical.

Example, the FM synth that Live's **Operator** imports into:

- From the manual: four oscillators (operators), each with its own envelope, frequency ratio and
  level; a set of algorithms that connect them; feedback; a filter, an LFO and a pitch envelope.
- Our device: an FM synth whose operator, algorithm and envelope model is a superset of that, so
  every Operator patch has a place to land. Its algorithms are our own table, with the ones
  Operator needs among them.
- DSP: operators, envelopes and feedback ported from MSFA (Apache-2.0), the engine inside Dexed;
  filters and LFOs from our own `dsp` crate.
- Import: a corpus series of Operator patches we make ourselves (one per algorithm, envelope
  shapes, ratios, feedback), each saved as a set and bounced, so the mapping is tested against
  real sound. Until the device exists, Operator comes in as a placeholder that keeps all its
  parameters.

The other devices, planned the same way (sources are candidates; each licence is checked when
the work starts):

| Imports from Live | Loopwerk device | DSP from |
|---|---|---|
| Utility, Auto Pan | utility, auto-pan | own |
| EQ Three | three-band DJ EQ with kills | own crossovers (Linkwitz-Riley) |
| Vinyl Distortion | vinyl distortion | own |
| Overdrive | overdrive | own, oversampled |
| Glue Compressor, Compressor | bus compressor, compressor | own, Airwindows (MIT) for reference |
| Multiband Dynamics | multiband dynamics, upward and downward | own crossovers and detectors |
| Delay, Echo | delays | own delay lines |
| Reverb, Hybrid Reverb | algorithmic and convolution reverb | Dragonfly Reverb (GPL-3.0), own convolution |
| Saturator, Roar | multi-stage saturation | own, oversampled |
| EQ Eight, Auto Filter | EQ, filter | own (`dsp` filters) |
| Operator | FM synth | MSFA (Apache-2.0) via Dexed |
| Wavetable | wavetable synth | Vital (GPL-3.0), under its own name |
| Drift, Analog | subtractive synth | own, Surge XT (GPL-3.0) for reference |
| Simpler, Sampler, Drum Rack | sampler, drum rack | own |
| Collision, Tension, Electric | physical-modelling synth | Mutable Instruments Rings and Elements (MIT) |

## Audio to MIDI

Turning audio into something playable from MIDI, offline (not on the audio thread), each as a
command with undo. Behaviour: [`docs/spec/audio-to-midi.md`](docs/spec/audio-to-midi.md).

- **Slice to a new MIDI track.** Cut an audio clip into slices (at warp markers, transients,
  bars or a note grid), put each slice on its own pad of a sampler or drum rack on a new MIDI
  track, and write a MIDI clip that plays the slices back in their original rhythm, warped
  timing kept if asked. Needs the sampler and drum rack devices; transients come from the
  stretcher's analysis that `media` already computes. Slicing presets are our own.
- **Convert to a new MIDI track:** four narrow detectors instead of one that tries to write
  every note of a mix, since each narrow task is far more reliable.
  - *Drums:* onsets classified into kick, snare and hi-hat by their spectrum, written as notes
    of a drum rack; more classes (toms, cymbals) later with a small model.
  - *Bass:* one note at a time in the low register (about 30–300 Hz); pitch tracking limited to
    that range, note starts taken from transients.
  - *Melody:* one note at a time across the whole range; pitch tracking (own, pYIN-style) plus
    note segmentation.
  - *Chords:* chord names rather than every note: pitch classes (chroma) matched against chord
    templates and smoothed over time, then written as a voicing; the bass detector gives the
    inversion.
  - Full polyphonic transcription (every note of a piano part) comes later, if at all. Candidate:
    Spotify's Basic Pitch (Apache-2.0) run in pure Rust through `tract`, licence and model
    weights checked first.
- **Using what the project knows:** every detector snaps to the project's tempo and warp grid,
  and chords and pitches can be held to the project's key, so results need less cleaning up.
- **Stem separation first:** separating a full mix into drums, bass and the rest before
  detecting is the largest single gain in quality. Candidate: Demucs (MIT), model weights
  checked first. Also offered on its own, as separating stems to new audio tracks.
- **Dropping audio onto a MIDI track** asks which conversion to run (drums, bass, melody,
  chords), and writes the result where the audio was dropped.
- **Order:** after the sampler and drum rack exist; slicing first (smallest, reuses the
  stretcher's analysis), then drums, bass, melody and chords, then stem separation. An ADR comes
  before the detectors.
- **Tests:** synthetic audio with known notes (rendered by the engine itself), so the result
  can be compared note by note.

## Before the repository goes public

- Clean-room documentation matches how the design was actually made, and `CONTRIBUTING.md`
  allows familiar workflow and region structure while forbidding copied visual design.
- Reference mockups redrawn from the design brief alone.
- Contributor process: DCO sign-off, a pull request template with a clean-room confirmation, CI
  running fmt, clippy and tests, and a channel for reporting IP concerns.
- Public descriptions name other DAWs only to describe workflow or file compatibility.

## Later, not ordered

FL Studio projects (`.flp`), automation, sends and returns in the mix (groups and the master
already apply volume and mute), MIDI controllers and Push (from Ableton's published Push interface
docs), Max for Live devices as placeholders, bridging Windows plug-ins.

**ARA** after VST3 hosting works in the sandbox: Arrangement first, since looping, quantised
Session clips do not fit ARA's linear timeline; Session view later. ADR before implementation.

Linux is the primary platform; Windows and macOS are supported.

## What works now

### M0 Foundation
- Rust workspace, GPL-3.0-or-later, CI (fmt, clippy `-D warnings`, tests) on Linux and Windows.
- Clean-room rules for people and agents ([`CONTRIBUTING.md`](CONTRIBUTING.md)) and the agent
  guide ([`AGENTS.md`](AGENTS.md)): no panics, no `unsafe`, real-time safety, sandboxed plug-ins.
- DAW-neutral project model: tracks and mixer channels apart, a pool of clip contents placed by
  clips, so per-track clips and shared patterns both fit
  ([ADR 0001](docs/decisions/0001-daw-neutral-model.md)).

### M1 Reading Live Sets
From Live Sets we authored ourselves in Live 12.4 ([format spec](docs/spec/als-format.md),
[corpus guide](docs/spec/als-corpus.md)):
- tempo and time signature (read from their automation default, where Live keeps them)
- MIDI, audio, group and return tracks; volume, pan, track on/off, sends, routing into groups
- scenes, session clip slots and arrangement clips
- MIDI clips with notes (velocity, release velocity, deactivated notes), start and end markers,
  loop, deactivated clips
- audio clips: file reference, warp on/off, warp markers, warp mode, clip gain, transpose
- device chains: VST2 and VST3 plug-ins with their id and state kept, Live's own devices as
  placeholders
- anything not understood yet is skipped with a warning; the project still opens

### M2 Playing
- `loopwerk-cli play` plays a set, or one launcher scene, on the default audio device; `render`
  writes a 32-bit float WAV. Both run the same engine code.
- MIDI notes play as sine tones until instruments arrive. Group and master volume and mute apply.

### M3 Commands and agents (in progress)
- Every edit is a command with an id, typed parameters and one undo step; failed or invalid edits
  leave the project unchanged ([ADR 0002](docs/decisions/0002-commands.md)).
- Commands so far: `commands.list`, `project.info`, `project.tracks`, `project.clips`,
  `transport.setTempo`, `track.setVolume`, `track.setPan`, `track.setMute`, `clip.move`,
  `edit.undo`, `edit.redo`.
- `loopwerk-cli mcp` serves them to agents over the Model Context Protocol, with file access
  only below granted roots ([`docs/mcp.md`](docs/mcp.md)).
- Still to do: more commands (notes, clips, sends, tracks), a native project format so edits can
  be saved, MCP resources.

### M4 Clip launching (in progress)
- The engine launches clips and scenes on a quantisation grid: bars or note values (triplets
  too, or none), following the time signature; 1 bar by default
  ([spec](docs/spec/clip-launching.md)).
- One clip per track; a clip loops until it is stopped or replaced, a clip without a loop plays
  once. Scenes launch their row, empty slots with a stop button stop their track, and a scene
  tempo changes the tempo at the launch.
- A track that launched a clip leaves its arrangement clips until "back to arrangement".
- Warped launched clips are time-stretched like arrangement clips, and re-pitched after a tempo
  change.
- Transport commands (`clip.launch`, `scene.launch`, `track.stop`, `launcher.*`,
  `transport.play`/`stop`) reach the audio thread through a lock-free queue:
  `loopwerk-cli play --launcher` takes them as typed lines, `loopwerk-cli mcp --audio` from
  agents, and `project_render` renders a script of them offline.
- Still to do: follow actions, launch modes, per-clip quantisation, reading the global
  quantisation from a Live Set, and checking the behaviour marked as hypotheses in the spec
  against Live.

### M5 Audio (in progress)
- Audio files (WAV, AIFF, FLAC, MP3, Ogg Vorbis) are decoded in parallel when a set opens.
- Audio clips play with clip gain, loops and fades; unwarped clips also with transpose.
- Warped clips are **time-stretched in real time** along their warp markers and keep their
  pitch ([ADR 0003](docs/decisions/0003-time-stretching.md)). An 80 s set with 58 warped clips
  opens in 3.1 s.
- Stretched transients stay on the grid: a fresh stretcher takes over before every strong onset,
  so stretched drum loops no longer flam against unstretched ones (within 2 ms, measured with
  click tests).
- Arrangement clips are stretched 0.5 s ahead on a worker thread; the audio thread only copies
  their frames (2 % of one core on the same set, worst block 1–8 ms of its 10.7 ms).
- Still to do: transpose on warped clips; launched clips stretched ahead too; a disk cache for
  the stretcher's analysis; Rubber Band as
  the `HighQuality` stretch mode.
