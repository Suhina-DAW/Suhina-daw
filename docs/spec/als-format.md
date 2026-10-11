---
description: The Live Set (.als) format as observed: container, tracks, clips, notes, audio, warp markers, devices
read_when: reading .als, importer, XML element, warp markers, time signature, devices, plug-in state, Utility, StereoGain, device parameters
type: spec
status: current
last_verified: 2026-10-11
covers:
  - crates/als/src/lib.rs
  - crates/als/tests/synthetic.rs
---

# Live Set (`.als`) format

What we have learned about the Live Set format, from Live Sets we authored ourselves (see
[`als-corpus.md`](als-corpus.md)). Written in our own words; no Ableton code or documentation text.

- **Observed with:** Live 12.4.6 (`Creator="Ableton Live 12.4.6"`, `MinorVersion="12.0_12402"`,
  `MajorVersion="5"`), Windows.
- **Corpus:** series 1, files `00`–`20` (MIDI clips, scenes, mixer); series 2, files `30`–`36` (audio clips); series 3, files `40`–`44` (devices, plug-ins); series 4, files `50`–`62` (time signatures, clip markers, deactivation, groups, unwarped audio); series 6, files `110`–`135` (Utility).
- Every statement marked **(hypothesis)** still needs a corpus file that proves it.

## Container

A `.als` file is gzip-compressed UTF-8 XML. The root element is `<Ableton>` with the version
attributes above; its only relevant child is `<LiveSet>`.

Most scalar values are stored as an element with a `Value` attribute, for example
`<Name Value="Test" />`. Automatable parameters are an element with a `<Manual Value="…"/>` child
(the current value when there is no automation) plus automation and MIDI-mapping data.

Many elements carry an `Id` attribute. Ids are unique only among siblings of the same kind and
are **not stable**: Live renumbers some of them on every save (see *Noise*), and moving an
arrangement clip gives it a new id. Never use them to identify an object across saves.

## Time

All musical times are in **beats** (quarter notes), as decimal numbers. Note `Time="1"` is beat 2
of bar 1; `Duration="1"` is a quarter note. Bars are not stored; at 4/4, bar 3 starts at beat 8.

Default automation values use the time `-63072000` as a "before everything" marker.

## Transport

| What | Where (under `LiveSet/MainTrack/DeviceChain/Mixer`) | Encoding |
|---|---|---|
| Tempo | `Tempo/Manual@Value` | BPM as a decimal number |
| Time signature | the default event of the time-signature automation envelope (below) | An integer code: `99 × log2(denominator) + numerator − 1`. Observed: 3/4 = `200`, 4/4 = `201`, 6/8 = `302`, 7/8 = `303`. |

Each parameter's current value also lives in the default event (`Time="-63072000"`) of its
automation envelope. The envelope is found through ids: the parameter's `AutomationTarget@Id`
equals `MainTrack/AutomationEnvelopes/Envelopes/AutomationEnvelope/EnvelopeTarget/PointeeId@Value`.
For the tempo, `Manual` and the default event change together (a `FloatEvent`). For the time
signature only the default event changes (an `EnumEvent`); `TimeSignature/Manual` stayed `201`
through files `50`–`53`. **Read the envelope first and fall back to `Manual`.**

## Tracks

`LiveSet/Tracks` lists the tracks in display order. Element names give the kind: `MidiTrack`,
`AudioTrack`, `ReturnTrack` and `GroupTrack`. The main output is the
separate `LiveSet/MainTrack`.

Per track:

| What | Where | Notes |
|---|---|---|
| Name | `Name/EffectiveName@Value` | Default names look like `1-MIDI`, `3-Audio`, `A-Reverb`. |
| Colour | `Color@Value` | Index into Live's colour palette. |
| Group | `TrackGroupId@Value` | `-1` when not in a group; otherwise the group track's `Id`. Grouping two tracks (file `60`) inserted a `GroupTrack` before them and renumbered the default names (`3-Audio` became `4-Audio`). |
| Mixer | `DeviceChain/Mixer` | see below |
| Output | `DeviceChain/AudioOutputRouting/Target@Value` | `AudioOut/Main` routes to the main output; `AudioOut/GroupTrack` to the track's group. |
| Session clips | `DeviceChain/MainSequencer/ClipSlotList/ClipSlot` | One slot per scene, in scene order. Return tracks have no slots. |
| Arrangement clips | `DeviceChain/MainSequencer/ClipTimeable/ArrangerAutomation/Events` | `MidiClip` (and `AudioClip`) elements. |

### Mixer

Under `DeviceChain/Mixer`, values in `…/Manual@Value`:

| What | Element | Encoding |
|---|---|---|
| Volume | `Volume` | Linear gain. `1` = 0 dB; `0.5011872053` = −6 dB. |
| Pan | `Pan` | `-1` … `1`; Live's "50L" is `-1` (full left). |
| Track on/off ("mute") | `Speaker` | `true` = playing, `false` = muted. |
| Sends | `Sends/TrackSendHolder[n]/Send` | Linear gain. One holder per return track, in return-track order. "−∞" in the UI is stored as `0.0003162277571` (−70 dB). |

## Scenes

`LiveSet/Scenes/Scene`, in display order. A new scene inserted in the middle gets the next free
`Id` but takes its place in the list, so **order comes from the list, not the ids**.

Per scene: `Name@Value`, `Tempo@Value` with `IsTempoEnabled@Value` (the tempo applies only when
enabled), and the same pair for time signature.

## MIDI clips

The same `MidiClip` element is used in session slots
(`ClipSlot/ClipSlot/Value/MidiClip`) and on the arrangement.

| What | Where | Notes |
|---|---|---|
| Arrangement position | `@Time` | Beats. Session clips have `Time="0"`. |
| Extent | `CurrentStart@Value`, `CurrentEnd@Value` | Arrangement clips: positions on the arrangement (`8` … `12` for a one-bar clip at bar 3). Session clips: positions in the clip's content (`0` … `4`). |
| Loop on/off | `Loop/LoopOn@Value` | |
| Loop region | `Loop/LoopStart@Value`, `Loop/LoopEnd@Value` | In content beats. Setting the clip length to 2 bars changed `LoopEnd` (and `CurrentEnd`) from 4 to 8. |
| Start marker | loop on: `LoopStart + StartRelative`; loop off: `LoopStart` | Beats. See *Markers* below. |
| End marker (loop off) | `Loop/LoopEnd@Value` | Not `OutMarker`, whose meaning is still unknown. |
| Name, colour | `Name@Value`, `Color@Value` | |
| Deactivated clip | `Disabled@Value` | `true` after selecting the clip and pressing `0` (file `58`). |

### Markers

Observed on a session clip with a 2-bar loop (files `54`–`57`), in beats:

| Edit | `LoopStart` | `LoopEnd` | `StartRelative` | `LoopOn` | `HiddenLoopStart`/`End` |
|---|---|---|---|---|---|
| before | 0 | 8 | 0 | true | 0 / 4 |
| Start → 1.2.1 | 0 | 8 | **1** | true | |
| Loop Position → 1.3.1 | **2** | **10** | **7** | true | |
| Loop off | **9** | 10 | **0** | **false** | **2 / 10** |
| End → 3.4.1 | 9 | **11** | 0 | false | 2 / 10 |

- With the loop on, the start marker is `LoopStart + StartRelative`. Moving the loop kept its
  length and moved the start marker by the same distance (to 3.2.1 = beat 9), so it stayed inside.
- Turning the loop off rewrote the fields: `LoopStart` became the start marker and `LoopEnd` the
  end marker, and the loop region moved to `HiddenLoopStart`/`HiddenLoopEnd` so it can return.
- For a session clip, `CurrentStart`/`CurrentEnd` follow: `CurrentEnd − CurrentStart` was the
  played length (8 with the loop on, then 1, then 2).

### Notes

Notes are grouped by key: `Notes/KeyTracks/KeyTrack`, each with `MidiKey@Value` (MIDI note
number) and its notes in `Notes/MidiNoteEvent`:

| Attribute | Meaning |
|---|---|
| `Time` | Start in content beats |
| `Duration` | Length in beats |
| `Velocity` | 0–127, **a decimal**: dragging gave `64.251297` |
| `OffVelocity` | Release velocity, 0–127 |
| `NoteId` | Per-clip note id |
| `IsEnabled` | Present as `false` on a deactivated note (file `59`); absent otherwise |

Live names MIDI note 60 "C3" (so 64 is E3, 67 is G3). Notes drawn without changing anything got
velocity 100 in one clip and 98 in another.

## Audio clips

Corpus: series 2, files `30`–`35` (one 4-second, 48 kHz WAV rendered by Loopwerk).

`AudioClip` shares `Time`, `CurrentStart`/`CurrentEnd`, `Loop`, `Name`, `Color` and `Disabled`
with `MidiClip`. Session audio clips sit in clip slots like MIDI clips, but **arrangement audio
clips are under `DeviceChain/MainSequencer/Sample/ArrangerAutomation/Events`**, not under
`ClipTimeable` as on MIDI tracks.

| What | Where | Notes |
|---|---|---|
| File | `SampleRef/FileRef/RelativePath@Value` | Relative to the Live Set's folder (`../loopwerk-demo.wav`). |
| File, absolute | `SampleRef/FileRef/Path@Value` | The full path on the machine that saved the set. **Personal data**: strip before sharing a corpus. |
| File size, checksum | `FileRef/OriginalFileSize`, `FileRef/OriginalCrc` | |
| Length | `SampleRef/DefaultDuration@Value` | In sample frames (`192000`). |
| Sample rate | `SampleRef/DefaultSampleRate@Value` | `48000` |
| Warp on/off | `IsWarped@Value` | |
| Warp mode | `WarpMode@Value` | `4` for this file by default, shown by Live as **Complex** (files `30`, `61`). Other codes **(needs corpus)**. |
| Warp markers | `WarpMarkers/WarpMarker@SecTime`, `@BeatTime` | Pairs of (seconds in the file, beats in the clip). |
| Clip gain | `SampleVolume@Value` | Linear, `1` = 0 dB; −6 dB is `0.5011872053` (file `36`). |
| Transpose | `PitchCoarse@Value`, `PitchFine@Value` | Semitones, and cents **(fine: hypothesis)**. |

**Warp markers.** A newly added file got markers at (0 s, beat 0) and (4 s, beat 8): Live assumed
120 bpm. Each list ends with one more marker 1/32 beat after the last real one
((4.015625 s, beat 8.03125)); it sets the playback rate after the last marker. Adding a marker on
beat 2 and dragging it right replaced the list with (0, 0), (0.500146 s, beat 1.125) and its
1/32-beat follower.

**Unwarped clips.** With warp off, the clip's loop fields are in **seconds**. Turning warp off
in file `31` changed `LoopEnd` from `8` (beats) to `4`; in file `61`, after a warp marker had been
moved, it became `3.9376458177`, which is beat 8 converted through the warp markers (0.500146 s at
beat 1.125, then 0.5 s per beat). Moving the start to 1 s (file `62`) set `LoopStart` to `1`. The
warp markers are kept in `SavedWarpMarkersForStretched` while warp is off. With warp off the loop
switch is unavailable in Live.
`OutMarker` and `CurrentEnd` did not change.

Adding the first clip to an empty track renamed the track after the clip (`4-Audio` →
`4-loopwerk-demo`), stored in `Name/EffectiveName` and `Name/MemorizedFirstClipName`.

## Devices

Corpus: series 3, files `40`–`44` (Simpler, EQ Eight, and the commercial plug-ins ShaperBox 3
and Sylenth1 as VST2 and VST3; the plug-in files stay local, see below).

A track's device chain is `DeviceChain/DeviceChain/Devices`, devices in chain order. The main
track has the same structure. Adding an instrument to a track named after its first clip renamed
it after the device (`1-MIDI` → `1-Simpler`, later `1-Sylenth1`).

Every device has `On/Manual@Value` (the device on/off switch; turning EQ Eight off changed it
from `true` to `false`) and `UserName@Value` (empty unless renamed).

**Live's own devices** are elements named after the device type: `OriginalSimpler`, `Eq8`,
`Reverb`, `Delay`. Their parameters are nested elements with the usual `Manual` values; band 3
gain of EQ Eight is `Bands.2/ParameterA/Gain/Manual` (+3 dB stored as `3`). `LastPresetRef`
points at the default preset the device was loaded from.

**Plug-ins** are `PluginDevice` elements whose `PluginDesc` holds one of:

| | `VstPluginInfo` (VST2) | `Vst3PluginInfo` (VST3) |
|---|---|---|
| Name | `PlugName@Value` | `Name@Value` |
| Identity | `UniqueId@Value`: the 32-bit plug-in id as a decimal | `Uid/Fields.0`…`Fields.3`: the 16-byte class id as four signed 32-bit decimals |
| State | `Preset/VstPreset/Buffer`: hex text | `Preset/Vst3Preset/ProcessorState` and `ControllerState`: hex text |
| Location | `Path@Value`: absolute path of the `.dll` on the saving machine (**personal data**) | none |
| Other | `Version`, `NumberOfParameters`, `Category`, window position | `DeviceType` (`1` for the instrument, `2` for the effect **(hypothesis)**), window position |

Plug-in state is the plug-in vendor's own format. Loopwerk keeps it verbatim and hands it back
to the plug-in. **Never commit a corpus file that contains a commercial plug-in's state.**

### Utility (`StereoGain`)

Corpus: series 6, files `110`–`135`, and two extra files at 50M and 50S. Those files were saved as a chain, each from the previous
one, not from the default, so a neighbour diff shows the step; `128` has both phase switches on.

Live's Utility is the element `StereoGain`. Parameters that can be automated hold their value in
`<Name>/Manual@Value` next to an `AutomationTarget`; the two plain elements hold it in `@Value`.

| Element | Stored | Observed | Loopwerk `utility` |
|---|---|---|---|
| `Gain` | linear gain | -12 dB → `0.2511886358`, +6 dB → `1.99526238`, -inf → `0`, fully up → `56.2341309` (+35 dB) | `gain` = 20·log10 of it, dB; `0` → the minimum (silence) |
| `Balance` | -1 … 1 | the knob shows 50L … 50R; typing `50L` stored `-1`, typing `100R` stopped at `1` | `balance`, the same number |
| `StereoWidth` | fraction | 0 % → `0`, 200 % → `1.99999988`, fully up → `4` (400 %) | `width`, the same number |
| `MidSideBalanceOn` | `@Value` true/false | the Width knob's Mid/Side Mode (right-click menu, not on the panel). Turning it on reset `StereoWidth` to `1` | when true: `width` = 1 and `midSide` from `MidSideBalance` |
| `MidSideBalance` | 0 … 2, linear in what the knob shows | on switching the mode on, the knob shows `0` and stores `1`; 100M `0`, 50M `0.5`, 50S `1.5`, 100S `2` | `midSide` = value − 1 (−1 only mid, +1 only side), only when `MidSideBalanceOn` is true |
| `Mono` | true/false | | `mono` |
| `BassMono` | true/false | | `bassMono` |
| `BassMonoFrequency` | Hz | 50 … 500, default 120 | `bassMonoFrequency` |
| `BassMonoAudition` | `@Value` true/false | the headphones button by the frequency; not automatable | `bassMonoListen` |
| `PhaseInvertL`, `PhaseInvertR` | true/false | `Ø L`, `Ø R`, independent | `invertLeft`, `invertRight` |
| `ChannelMode` | 0 … 3 | `0` Left, `1` Stereo (default), `2` Right, `3` Swap | `channel`: Stereo 0, Left 1, Right 2, Swap 3 (a different order) |
| `DcFilter` | true/false | | `dcFilter` |
| `Mute` | true/false | | `mute` |
| `On` | true/false | the device switch, as on every device | the model's `Device::enabled` |
| `LegacyGain`, `LegacyMode` | `0`, `false` | unchanged in every file | not mapped **(hypothesis: older sets stored gain here)** |

Still open: how Mid/Side between the ends scales mid and side in the sound (the stored value is
linear in the display; Loopwerk keeps one at full and turns the other down linearly), and
whether Width and Mid/Side sound like Loopwerk's at the same settings. Both need bounces of
`118` and of the 50M and 50S files (`120-utility-midside-mid-50M.als`, `…-50S.als`).

## Noise: ignore when importing or diffing

These change without any musical edit:

- `LiveSet/NoteAlgorithms`: state of the MIDI tools; renumbered on every save.
- Everything named `ViewState*`, `SequencerNavigator`, `ScrollerTimePreserver`, `TimeSelection`,
  `ClientSize`, `ScrollerPos`, `CurrentZoom`, `HighlightedTrackIndex`, `Selected*`,
  `IsContentSelectedInDocument`, `LastSelected*`: view and selection state.
- `ColorSequenceIndex`, `AutoColorPicker*/NextColorIndex`: automatic colour assignment.
- `TakeId` on clips.
- `Transport/CurrentTime`: the play position.

## Open questions (next corpus files)

- Note probability and velocity deviation; MIDI CC and pitch-bend envelopes.
- Warp modes (`WarpMode` codes), audio fades.
- Return tracks added or removed; sends routed pre-fader; track output to another track.
- Tempo and time-signature changes over time (automation with more than the default event).
- CLAP plug-ins, Audio Units, racks, Max for Live devices.
- Sets saved by older Live versions.
