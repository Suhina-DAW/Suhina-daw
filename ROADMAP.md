# Roadmap

Suhina is pre-alpha. Milestones are in order; each one builds on the previous.

| | Milestone | Status |
|---|---|---|
| M0 | **Foundation:** workspace, GPL-3.0-or-later, clean-room rules, DAW-neutral model | done |
| M1 | **First sound:** offline renderer and the `.als` reader | done |
| M2 | **Real-time playback:** audio output, `suhina-cli play` | done (verified on Windows) |
| M3 | **Commands and agents:** command registry with undo; CLI and MCP server on the same commands | in progress: commands, undo, MCP, edits heard while playing, and saving projects in Suhina's own format (`.locr`) work; more commands to come |
| M4 | **Clip launching:** launch quantisation, scenes, stop buttons, follow actions | in progress: launching, scenes, stops, quantisation and the metronome work; follow actions and launch modes to come |
| M5 | **Audio playback:** imported audio clips with gain, transpose, warping and time-stretching | done (verified by ear on a heavily warped set) |
| M6 | **Plug-ins:** CLAP hosting, sandboxed out of process (AGENTS.md rule 5, ADR 0006); VST3 after, and projects with VST2 plug-ins opened through their VST3 versions | in progress: CLAP and VST3 effects load, process, keep state and survive crashes in their own process; installed plug-ins are found in seconds to minutes; `suhina-cli plugin run` puts a file through one; `suhina-cli plugins check` finds the installed plug-in for each in a project, VST3 versions for VST2 ones included, and `suhina-cli plugin run-device` runs one with the settings the project saved, an instrument playing its track's notes, `suhina-cli plugin params` lists a plug-in's parameters, and `suhina-cli plugin edit` opens its editor (Windows, Linux X11); in the engine: imported plug-in effects and instruments play in the app and in renders, with latency compensation, a crashed one leaves the project playing |
| M7 | **More formats:** DAWproject import and export | |
| M8 | **UI:** Session and Arrangement views on top of the commands; familiar workflow, Suhina's own look; installers (AppImage, deb, Flatpak, MSI) and signed macOS and Windows builds | started |

## Built-in devices

Instruments and effects that ship with Suhina. They run inside the engine (rule 5 covers
third-party plug-ins only), so they follow the real-time and never-crash rules like the engine
itself. Written in Rust; DSP from GPL-compatible open-source projects may be ported (rule 9) with
its copyright notices kept and credited in [`ATTRIBUTION.md`](ATTRIBUTION.md). An ADR comes before
the first device.

- **Crates, in this repository:** `dsp` (filters, oscillators, envelopes, delay lines,
  convolution, oversampling; no engine or model dependency), `device-api` (the device trait,
  parameter descriptions, state) and one crate per device under `devices/`. Every parameter is a
  command and can be automated, from the UI, CLI and MCP alike. Because devices do not depend on
  the engine, they can later be split into their own repository or built as CLAP plug-ins.
  The interface is [ADR 0004](docs/decisions/0004-built-in-devices.md).
- **Status:** `dsp`, `device-api`, the utility, the auto-pan, the DJ EQ, the vinyl distortion, the
  overdrive, the compressor, the limiter and the FM synth (the first instrument: the DX7-style
  engine ported from MSFA, with DX7 voice files) exist and are tested on their own. The engine
  runs them in chains on tracks: a track's sound goes through its devices before its fader, and
  an instrument first in the chain plays the track's notes (LOOP-35); `device.*` commands add,
  remove, switch and set them.
  Chains on group and master channels, and automation, come next.
- **Importing devices from Live Sets, in three levels:** a placeholder that keeps the parameters;
  then an approximate mapping to an open-source plug-in over CLAP; then a Suhina device for the
  most used ones. After import, the user sees which devices came in exactly, approximately or as
  placeholders. No content from Live's library (presets, wavetables, samples, impulse responses).
- **Order:** utility (gain, width, mono, phase) and auto-pan first, to prove the device system;
  then three small devices users have asked for: a three-band DJ EQ with kills, vinyl distortion
  (tracing, pinch, crackle) and a pedal-style overdrive; then bus compressor, multiband dynamics with upward and downward compression, delays,
  algorithmic and convolution reverb, multi-stage saturation; then an FM synth (DSP core ported
  from Dexed/MSFA) and a wavetable synth (from Vital, under its own name).

### How each device is built

Every Suhina device follows the same path:

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

Every device, planned the same way (sources are candidates; each licence is checked when the
work starts). **DSP done** means the device exists in `devices/` and passes its tests, but the
engine does not run it yet; **import** is whether a Live Set's device comes in as it (otherwise a
placeholder):

| Imports from Live | Suhina device | DSP from | Status |
|---|---|---|---|
| Utility, Auto Pan | utility, auto-pan | own | DSP done; import done for Utility, planned for Auto Pan |
| EQ Three | three-band DJ EQ with kills | own crossovers (Linkwitz-Riley) | DSP done; import planned |
| Vinyl Distortion | vinyl distortion | own, oversampled | DSP done; import planned |
| Overdrive | overdrive | own, oversampled | DSP done; import planned |
| Compressor | compressor: stereo-linked, soft knee, side-chain high-pass, dry/wet | own | DSP done; import planned |
| Glue Compressor | bus compressor | own, Airwindows (MIT) for reference | planned |
| Limiter | limiter: brickwall, 1.5 ms lookahead | own | DSP done; import planned |
| Multiband Dynamics | multiband dynamics, upward and downward | own crossovers and detectors | planned |
| Delay, Echo | delays | own delay lines | planned |
| Reverb, Hybrid Reverb | algorithmic and convolution reverb | Dragonfly Reverb (GPL-3.0), own convolution | planned |
| Saturator, Roar | multi-stage saturation | own, oversampled | planned |
| EQ Eight, Auto Filter | EQ, filter | own (`dsp` filters) | planned |
| Operator | FM synth | MSFA (Apache-2.0) via Dexed | DX7-style engine done; Operator's algorithms, waveforms and filter, and import, planned |
| Wavetable | wavetable synth | Vital (GPL-3.0), under its own name | planned |
| Drift, Analog | subtractive synth | own, Surge XT (GPL-3.0) for reference | planned |
| Simpler, Sampler, Drum Rack | sampler, drum rack | own | planned |
| Collision, Tension, Electric | physical-modelling synth | Mutable Instruments Rings and Elements (MIT) | planned |

## Audio to MIDI

Turning audio into something playable from MIDI, offline (not on the audio thread), each as a
command with undo. Behaviour: [`docs/spec/audio-to-midi.md`](docs/spec/audio-to-midi.md).

- **Slice to a new MIDI track.** Cut an audio clip into slices (at warp markers, transients,
  bars or a note grid), put each slice on its own pad of a sampler or drum rack on a new MIDI
  track, and write a MIDI clip that plays the slices back in their original rhythm, warped
  timing kept if asked. Needs the sampler and drum rack devices and an onset detector of our
  own. Slicing presets are our own.
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
- **Order:** after the sampler and drum rack exist; slicing first (smallest), then drums, bass, melody and chords, then stem separation. An ADR comes
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
docs), Max for Live devices kept as placeholders and copied along with the project, bridging Windows
plug-ins.

**Patch devices:** devices built by the community as Pure Data patches, with access to
commands; plugdata works as an ordinary plug-in once hosting lands
([ADR 0008](docs/decisions/0008-patch-devices.md), proposed).

**ARA** after VST3 hosting works in the sandbox: Arrangement first, since looping, quantised
Session clips do not fit ARA's linear timeline; Session view later. ADR before implementation.

Linux is the primary platform; Windows and macOS are supported.

## What works now

### M0 Foundation
- Rust workspace, GPL-3.0-or-later, CI (fmt, clippy `-D warnings`, tests) on Linux for every change;
  Windows and macOS when started by hand and in every release build.
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
- `suhina-cli play` plays a set, or one launcher scene, on the default audio device; `render`
  writes a 32-bit float WAV. Both run the same engine code.
- Built-in devices on a track run in a chain before its fader; MIDI notes play on the track's
  instrument when the chain starts with one (the FM synth), otherwise as sine tones. Group and
  master volume and mute apply. A group with devices is a bus: its devices run over the sum of
  its tracks, with latency compensation on every path. `--fm` on `play` and `render` puts the FM synth on every MIDI
  track, with a voice from a DX7 file; `suhina-cli fm-synth voices` lists a file's voices.

### M3 Commands and agents (in progress)
- Every edit is a command with an id, typed parameters and one undo step; failed or invalid edits
  leave the project unchanged ([ADR 0002](docs/decisions/0002-commands.md)).
- Commands so far: `commands.list`, `project.info`, `project.save`, `project.tracks`, `project.clips`,
  `transport.setTempo`, `track.setVolume`, `track.setPan`, `track.setMute`, `track.setSolo`, `track.setOutput`, `track.group`, `track.ungroup`, `track.move`, `track.rename`,
  `track.setColor`, `track.delete`, `clip.move`, `clip.duplicate`, `clip.delete`, `clip.setGain`, `clip.setFades`, `clip.copy`, `clip.cut`, `clip.paste`,
  `edit.undo`, `edit.redo`; for the transport `transport.play`, `transport.stop`,
  `transport.setPosition`, `transport.setMetronome`; for several open projects `projects.list`, `projects.switch`,
  `projects.next`, `projects.previous`, `projects.close`; for plug-ins `plugins.scan`,
  `plugins.list`, `plugins.folders`, `plugins.addFolder`, `plugins.removeFolder`,
  `plugins.check`, `plugins.setHosting` (also as `suhina-cli plugins …`); for the audio
  output `audio.devices` (also `suhina-cli audio devices`), `audio.setOutput`, `audio.info`;
  for devices
  `devices.list`, `track.devices`, `device.add`, `device.remove`, `device.setEnabled`,
  `device.setParam`, `device.addPlugin`, `device.plugin`, `device.openEditor`,
  `device.closeEditor`, `device.restart`; for tracks and files `track.add`, `clip.addFiles`
  (samples and MIDI files as clips); for previews `preview.play`, `preview.stop`; for the
  browser's library `library.folders`, `library.addFolder`, `library.removeFolder`,
  `library.refresh`, `library.list`, `library.search`, `library.info`, `library.pinned`,
  `library.pin`, `library.unpin`, `library.recent`, `library.tags`, `library.setTag`,
  `library.removeTag`, `library.tag`; for the sample map `soundmap.status`, `soundmap.analyse`,
  `soundmap.layout`, `soundmap.similar`, `soundmap.info`; for missing files `project.missingFiles`,
  `library.findFiles`, `project.relinkFiles`.
- Several projects open at once, like tabs, one of them active and playing; commands go to the
  active one or to a named one ([ADR 0007](docs/decisions/0007-several-open-projects.md)).
- `suhina-cli mcp` serves them to agents over the Model Context Protocol, with file access
  only below granted roots ([`docs/mcp.md`](docs/mcp.md)).
- Edits and undo/redo are heard by a playing engine from its next audio block, without a gap:
  a new graph is swapped in at an exact frame and playing clips carry on; device parameters go
  as messages, without a new graph
  ([ADR 0005](docs/decisions/0005-edits-while-playing.md)).
- Projects are saved in Suhina's own format, `.locr` (versioned JSON, plug-in state included,
  audio referred to by path): `project.save { path?, copy? }`; `.locr` files open in the app,
  `suhina-cli` and MCP like Live Sets do ([ADR 0010](docs/decisions/0010-project-files.md),
  [spec](docs/spec/locr-format.md)).
- Still to do: more commands (notes, clips, sends), MCP resources, asking before closing
  a project with unsaved changes.

### M4 Clip launching (in progress)
- The engine launches clips and scenes on a quantisation grid: bars or note values (triplets
  too, or none), following the time signature; 1 bar by default
  ([spec](docs/spec/clip-launching.md)).
- One clip per track; a clip loops until it is stopped or replaced, a clip without a loop plays
  once. Scenes launch their row, empty slots with a stop button stop their track, and a scene
  tempo changes the tempo at the launch.
- A track that launched a clip leaves its arrangement clips until "back to arrangement".
- A metronome (`transport.setMetronome`, off by default): a click on every beat while the
  transport runs, higher on the first beat of a bar, following tempo edits and scene tempos.
  Count-in comes with recording.
- Warped launched clips are time-stretched like arrangement clips, and re-pitched after a tempo
  change.
- Transport commands (`clip.launch`, `scene.launch`, `track.stop`, `launcher.*`,
  `transport.play`/`stop`/`setPosition`) reach the audio thread through a lock-free queue:
  `suhina-cli play --launcher` takes them as typed lines, `suhina-cli mcp --audio` from
  agents, and `project_render` renders a script of them offline.
- Still to do: follow actions, launch modes, per-clip quantisation, reading the global
  quantisation from a Live Set, and checking the behaviour marked as hypotheses in the spec
  against Live.

### M5 Audio (done)
- Audio files (WAV, AIFF, FLAC, MP3, Ogg Vorbis) are decoded in parallel when a set opens.
- Audio clips play with clip gain, transpose, loops and fades; on warped clips transpose changes
  the pitch only.
- Warped clips are **time-stretched in real time** along their warp markers and keep their
  pitch, with our pure-Rust port of Signalsmith Stretch
  ([ADR 0003](docs/decisions/0003-time-stretching.md)). An 80 s set with 58 warped clips opens
  in 0.4 s. Transients stay within 2 ms of the warp markers, so stretched drum loops do not flam
  against unstretched ones.
- Arrangement clips are stretched 0.5 s ahead on a worker thread; the audio thread only copies
  their frames (3 % of one core on the same set, 99 % of blocks within 1.3 ms of their 10.7 ms).
- Later, not needed for M5: launched clips stretched ahead too; formant shifting for transposed
  vocals; Rubber Band for the `HighQuality` mode if a listening test shows it is worth it.

### M6 Plug-ins (in progress)
- CLAP and VST3 plug-ins run in `suhina-plugin-host` processes, never in Suhina's: by
  default one process per plug-in file, or per vendor, or one per instance
  (`plugins.setHosting`); a crash or hang costs that plug-in's sound, not the project
  ([ADR 0006](docs/decisions/0006-plug-in-hosting.md)).
- An opened Live Set's plug-ins play in the app: each is found among the installed ones (a VST3
  version for a VST2 plug-in too) and started with the settings the project saved; instruments
  play their track's notes, effects process their track. Missing ones are named in the status
  bar and keep their settings untouched.
- While an opened project's plug-ins start, a dialog shows how many are ready and which are
  still loading or could not start; it can be sent to the background, and the project plays
  meanwhile. A scan finds plug-ins that were missing without opening the project again.
- Tracks are lined up for the latency of their devices and plug-ins.
- The device panel shows each plug-in's state (loading, running, missing, crashed) with Open
  editor, Close editor or Restart; the same commands work from the CLI and MCP. Editors float
  above Suhina's window, owned by it (Windows, X11), and respond at once; a setting
  (Plug-ins › Editors › Device view) shows them inside the device panel instead, as part of
  the chain. On Linux the window is an X11 one whenever an X server (or XWayland) is there,
  so this works on Wayland desktops too.
- Renders (`suhina-cli render`, MCP `project_render`) run the same plug-ins.
- The plug-ins of different tracks run at the same time (one block out to every host, then
  every answer collected), the engine renders ahead of the audio device in blocks of its own,
  and host audio threads run at real-time priority: a project with 25 plug-ins plays without a
  late block. The device panel says when a plug-in does fall behind ("N% of blocks late").
- Still to do: plug-ins of one process in one round trip, plug-in parameters as automation,
  Wayland and macOS editors, VST2 hosting (not decided).

### M8 UI (started)
- `suhina` opens a Live Set (as an argument, or dropped on the window) or `--demo`, plays it
  stopped on the default output and shows the **Session view**: tracks as columns, return tracks
  beside the scene column, clip slots with their state (stopped, queued, playing with progress),
  scene launch, stop buttons, and a mixer strip with pan, volume and track on/off
  ([ADR 0009](docs/decisions/0009-ui-toolkit-and-look.md), [spec](docs/spec/ui-session-view.md)).
- The **Arrangement view** (Tab switches): bar ruler, one lane per track with its clips and their
  notes (loops repeated), the playhead, track headers on the right that show volume and pan once
  a lane is tall enough (drag its lower edge), return and main rows along the bottom. Clips move
  by dragging (`clip.move`), snapped to a grid that follows the zoom (Ctrl and the wheel).
- A click on the ruler sets where playback starts (`transport.setPosition`, on the clip grid,
  Alt for free), shown as a marker; while playing, playback jumps there. Stop returns to the
  marker, or stays where it stopped (Settings › General › Stop); a second Stop goes to the
  beginning ([spec](docs/spec/arrangement-editing.md)).
- Clips move between tracks by dragging them up or down (`clip.move` with `to_track`); a MIDI
  clip then plays through the new track's instrument and effects.
- **New, open and save:** Ctrl+T shows a dialog to make a new blank project, one from a template
  (the templates folder is a setting) or open one (a typed path or the recent ones); Ctrl+S saves
  as `.locr` (asking for a name and folder the first time), Ctrl+Shift+S saves as, and File ›
  Save as template writes a copy to the templates folder. The tab and window title mark unsaved
  changes with a dot.
- The **browser** (Ctrl+B shows and hides it, left or right of the view): search (Ctrl+F),
  category filters (All, Samples, MIDI, Instruments, Effects, Presets, Plug-ins), a sidebar of
  Pinned, Recent (by kind) and the folders the user added (a path, or a folder dropped on the
  window), a tree with type icons, lengths and colour tags, a preview strip with the waveform,
  length, tempo and key, and a footer with the tags as filters. Samples and MIDI files preview
  through the engine (auto-play, or the play button; MIDI through the selected track's
  instrument), whether the project plays or not. Dragging samples and MIDI files onto a lane,
  a slot or the empty space after the tracks places them (`clip.addFiles`; one track, or one
  each with Ctrl; a sample with a known tempo is warped; a Stems file `.stem.mp4` becomes a group
  of its stems and its muted master, with its mastering compressor and limiter on the group),
  devices, DX7 presets and plug-ins onto
  a track or the empty space add them (`device.add`, `track.add`); Enter or a double-click adds
  to the selected track. The folders are indexed on disk and watched for changes
  ([spec](docs/spec/browser.md), [ADR 0011](docs/decisions/0011-preview-and-added-audio.md),
  [ADR 0012](docs/decisions/0012-file-library.md)).
- **Missing files**: a project whose audio files cannot be found says so in a bar when it opens
  (Find…, Later; View › Missing files). The panel lists them with the clips that use them, finds
  them by name in the browser's folders and the project's folder at once, or in a folder typed or
  dropped on it (walked in the background), best match first; Apply relinks them in one undo step
  ([spec](docs/spec/missing-files.md)).
- The **sample map** (the browser's List | Map switch, or large in the main area): the samples
  of the browser's place laid out by how they sound, coloured by kind (kick, snare, clap, hats,
  toms, cymbals, percussion, bass, tonal, vocal, effect; from the name or guessed from similar
  samples), as a free cloud or snapped to a grid, with the kinds' names over their regions and a
  legend that filters. Pointing at a sample previews it; click, Ctrl+click, Shift+drag and the
  arrow keys select; a drag places it like a drag from the list; right-click › Show similar rings
  and lists the 16 that sound most alike. Samples are analysed in the background once and kept
  ([spec](docs/spec/sample-map.md), [ADR 0013](docs/decisions/0013-sample-map-analysis.md)).
- A menu bar and a **Settings** page (Options › Settings, Ctrl+,) with its pages listed down the
  left: General, Interface (with Theme, Arrangement, Detail area and Browser under it), Graphics, Audio,
  Shortcuts, Plug-ins; animations, which side the track headers are on, the renderer (wgpu or
  OpenGL); saved in `settings.txt`.
- The whole window zooms from 50 % to 200 % (Ctrl and + or -, Cmd on macOS; Ctrl and 0 for
  100 %; View › Zoom in and Settings › Interface › Zoom), and the zoom is kept for the next run
  ([spec](docs/spec/window-zoom.md)).
- **Themes**: every colour comes from the theme in use, a JSON file in the `themes` folder
  (`{ "name": …, "colors": { "accent": "#A6A3FF", … } }`). Nine come with Suhina (Suhina
  Dark and Light, Editor Dark, High Contrast, Dracula, Nord, Catppuccin Mocha and Latte, Solarized
  Dark). Settings › Interface › Theme picks one, changes its colours, renames, duplicates, reads
  the files again, backs every theme up into a zip and exports a template zip with a README an AI
  agent can make a theme from; a theme file or backup dropped on the window is added, and a file
  saved in an editor shows within a second.
- Short animations (hover fades, a pulsing queued clip, values easing after an undo, the view
  switch), which Settings turns off.
- The control bar has tempo, launch quantisation, play, stop and the position; Space plays and
  stops, Ctrl+Z and Ctrl+Shift+Z undo and redo. Every click is a command, the same the CLI and
  MCP run.
- Solo buttons in the Session mixer and the Arrangement headers (`track.setSolo`): exclusive by
  default, Ctrl adds; the engine hears soloed tracks, their groups and the return tracks.
- A track header 70 px or taller shows where the track plays: the main output or a group
  (`track.setOutput`, loops refused).
- A clip dropped over others replaces what it covers (shortens, splits or removes them), or
  stacks with them when Allow sample / MIDI stacking is on; stacked clips share the lane in
  sub-rows ([spec](docs/spec/arrangement-editing.md)).
- Tracks: click a header to select it (Ctrl adds), Ctrl+G groups the selection, drag a header
  to move the track (a group moves with its tracks); right-click a header or a Session column for
  name, colour, group, ungroup and delete. Clips: right-click or Delete and Ctrl+D.
- Audio clips show their waveform (peaks computed once when the files are decoded) and their
  gain, which a drag changes (`clip.setGain`); the waveform follows the gain while it is dragged.
- Audio clips on the arrangement fade in and out: handles at a clip's top corners set the fades
  on the grid (Alt for free, double-click removes), the fades are drawn over the waveform, and
  the engine plays them sample-accurately in renders and playback (`clip.setFades`,
  [spec](docs/spec/arrangement-editing.md#fades)).
- Several projects in **tabs** (ADR 0007): a tab row once two are open, Ctrl+Tab between them,
  only the active one plays; opening one while another is open asks (new tab or replace, with
  Remember and a setting). A project file dragged over the window darkens it.
- Settings › Audio: the driver (WASAPI, or ASIO in a build with the `asio` feature), the output
  device, sample rate and buffer size as dropdowns of what the driver allows, WASAPI exclusive
  mode (off by default), and what plays now with its measured latency and underruns. A change
  reopens the output with the transport where it was and the plug-ins running; the engine
  renders in blocks of the buffer size, so the plug-ins' deadline follows it. A device that is
  gone at start plays the default output, with a notice. `suhina-cli play` takes the same
  choice (`--host`, `--device`, `--buffer`, `--exclusive`).
- Settings › Plug-ins: the folders scanned (add, remove), Scan and Rescan all in the background,
  and the plug-ins found and the files that failed.
- Every key is the user's to change: Settings › Shortcuts and `keymap.json` bind any command
  that needs no parameters, the actions on the selection (copy, cut, paste, duplicate, delete,
  group) and the window's own ([shortcuts](docs/shortcuts.md)).
- A **detail area** under the view (or a column beside it, Settings › Interface › Detail area) shows the selected
  clip's properties or the selected track's devices with their parameters, and each plug-in's
  state with a button that opens its editor or restarts it; Shift+Tab hides it.
  Changing devices from it follows the device commands (#73).
- Still to do: fades read from Live Sets, input routing and sends, arm, meters, device editing
  and the clip editor in the detail area, installers.
