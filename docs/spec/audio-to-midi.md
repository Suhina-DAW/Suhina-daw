---
description: How slicing audio to a MIDI track and converting drums, bass, melody and chords to MIDI behave
read_when: slice to MIDI, audio to MIDI, convert drums, bass, melody, chords, transcription, drop audio on MIDI track
type: spec
status: planned
last_verified: 2026-10-14
---

# Audio to MIDI

- **Sources:** the maintainer's screenshots of Live 12.4 (2026-10-11): the audio clip context menu,
  the *Slice to New MIDI Track* dialog with its open menus, the *Convert Audio To MIDI* dialog,
  and dragging an audio clip onto another track in the Arrangement. The public Live 12 manual
  for what the commands produce.
- **Observed with:** the screenshots only; nothing has been run or measured yet. Statements
  marked **(hypothesis)** need checking (see *To observe*).
- **Status:** behaviour spec only; nothing is implemented. Plan: [`ROADMAP.md`](../../ROADMAP.md)
  › Audio to MIDI.

This document describes the behaviour Suhina will implement, in Suhina's terms. Where
Suhina decides something the sources do not settle, the text says so (**Suhina:**). Live's
slicing presets, instruments and their sounds are its content and are not reproduced.

## Observed: the commands

The context menu of an audio clip has, together:

- *Slice to New MIDI Track*
- *Convert Harmony to New MIDI Track*
- *Convert Melody to New MIDI Track*
- *Convert Drums to New MIDI Track*
- *Separate Stems to New Audio Tracks* (stem separation; out of scope here)

## Slicing

### Observed

*Slice to New MIDI Track* opens a dialog with:

- **Create one slice per:** *Warp Marker*, *Transient*, *Bar*, *1/2 Note*, *1/4 Note* (selected
  by default in the screenshot), *1/8 Note*, *1/16 Note*, *1/32 Note*.
- A line that states the length of the clip region and how many slices the choice gives (an
  8-beat region at 1/4 note: 8 slices). It updates with the choice. **(hypothesis)**
- **Slicing Preset:** a list of named presets, the first one *Default*. The presets decide which
  instrument the new track gets and how it is set up (drum-oriented, low-end, vocal,
  MPE variants).
- **Preserve warped timing:** an on/off switch, on in the screenshot.
- *OK* and *Cancel*.

### Behaviour

1. **Slice points.** The clip region (start to end marker, or the loop region if the loop is on
   **(hypothesis)**) is cut at:
   - every warp marker, or
   - every detected transient, or
   - every bar, or every note value of the chosen grid, counted in beats from the region start.
   The number of slices is shown before the user confirms.
2. **New track.** A new MIDI track is created next to the source track, holding an instrument
   with one slice per pad or key, ascending from one start note. **(hypothesis:** the start
   note and the instrument depend on the preset.**)**
3. **New clip.** A MIDI clip of the same length and position as the audio clip plays each
   slice's note at the slice's position, so it sounds like the original.
4. **Preserve warped timing on:** each slice plays at the speed the warped clip played it, so
   the result keeps the clip's warping. **Off:** each slice plays at the speed of the file.
   **(hypothesis)**
5. The source audio clip is left as it is.

**Suhina:** presets are our own, starting with one per instrument (drum sampler, sampler). A
slice with no audible content is still created, so note numbers stay predictable.

## Converting

### Observed

- The three *Convert … to New MIDI Track* commands each create a new MIDI track with a MIDI clip.
  **(hypothesis** for the exact track and instrument they add.**)**
- **Dropping an audio clip onto a MIDI track** opens a dialog *Convert Audio To MIDI*: "What type
  of audio material are you converting?" with three choices, *Harmony*, *Melody* and *Drums*,
  each with a small picture of typical notes, and *Cancel*.

### Behaviour

**Suhina:** four kinds of material instead of three. A detector that writes every note of a
mix ("harmony") mixes parts up; narrow detectors are far more reliable, so Suhina offers:

1. **Drums:** each hit becomes a note on a fixed key per drum class: kick, snare, hi-hat to
   start with.
2. **Bass:** one note at a time in the low register (about 30–300 Hz). Each note gets its pitch,
   start and length; starts come from transients, since low pitches need long analysis windows.
3. **Melody:** one note at a time across the whole range. Each note gets the pitch heard, its
   start and its length; pitch glides become separate notes.
4. **Chords:** one chord at a time, recognised by its pitch classes and written as a voicing in
   a middle register, with the bass note from the bass detector as the inversion. Chords change
   at most once per beat.

For all four:

- **Velocity** follows the loudness of each note or hit.
- **Grid and key:** note starts snap to the project's tempo and warp grid within a tolerance,
  and pitches and chords can be held to the project's key.
- **Dropping onto a MIDI track** offers these four choices and *Cancel*. After a choice, the
  MIDI clip is written on the track it was dropped on, at the drop position, and the audio clip
  is not placed there. *Cancel* drops nothing. **(hypothesis** for Live; Suhina does this.**)**
- Every conversion is a command (`clip.sliceToMidi`, `clip.convertToMidi` with
  `material: drums | bass | melody | chords`), runs off the audio thread, and is one undo step.

### How the detectors work (Suhina)

Planned, not built; the ADR that comes before the detectors decides the details.

- **Slicing** needs an onset detector of our own (for *Transient*), and the sampler and drum
  sampler devices.
- **Drums:** onsets classified into kick, snare and hi-hat by their spectrum; more classes
  (toms, cymbals) later, with a small model.
- **Bass:** pitch tracking limited to the low range, note starts taken from transients.
- **Melody:** pitch tracking of our own (pYIN-style) plus note segmentation.
- **Chords:** pitch classes (chroma) matched against chord templates and smoothed over time.
- **Full polyphonic transcription**, if ever: candidate Spotify's Basic Pitch (Apache-2.0) run in
  pure Rust through `tract`, its licence and model weights checked first.
- **Stem separation:** its own spec, [`stem-separation.md`](stem-separation.md): a model the user
  supplies, since no widely used model's weights may be shipped with Suhina
  ([#146](https://github.com/suhina-daw/suhina-daw/issues/146)).

### Tests

Synthetic audio with known notes, rendered by the engine itself, so a result can be compared
note by note.

For comparison, Live offers three: *Harmony* (several notes at once), *Melody* and *Drums*;
which keys its drum classes land on is in its manual. **(hypothesis)**

## To observe

Checks in Live 12.4 (screen recording or notes; no audio from Live is committed):

- Does the slice-count line update with each choice, and what does *Transient* count on a
  drum loop?
- Is the loop region or the start-to-end region sliced when the loop is on?
- Which note does the first slice land on, and on what instrument, with the *Default* preset?
- Preserve warped timing off: does each slice play at the file's speed?
- After *Convert Drums*: which keys hold kick, snare and hi-hat?
- Dropping audio onto a MIDI track: where does the clip land, and does the track get an
  instrument if it has none?
- Dropping audio onto a MIDI track that already has a clip at that place: is it replaced?

## Not yet

- Stem separation (*Separate Stems to New Audio Tracks*), planned as a step before detection
  and as a command of its own: [`stem-separation.md`](stem-separation.md).
- Full polyphonic transcription (every note of a mix or a piano part).
- Converting in real time or while recording.
