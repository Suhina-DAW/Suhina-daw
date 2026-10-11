# Audio to MIDI

- **Sources:** the owner's screenshots of Live 12.4 (2026-10-11): the audio clip context menu,
  the *Slice to New MIDI Track* dialog with its open menus, the *Convert Audio To MIDI* dialog,
  and dragging an audio clip onto another track in the Arrangement. The public Live 12 manual
  for what the commands produce.
- **Observed with:** the screenshots only; nothing has been run or measured yet. Statements
  marked **(hypothesis)** need checking (see *To observe*).
- **Status:** behaviour spec only; nothing is implemented. Plan: [`ROADMAP.md`](../../ROADMAP.md)
  › Audio to MIDI.

This document describes the behaviour Loopwerk will implement, in Loopwerk's terms. Where
Loopwerk decides something the sources do not settle, the text says so (**Loopwerk:**). Live's
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

**Loopwerk:** presets are our own, starting with one per instrument (drum rack, sampler). A
slice with no audible content is still created, so note numbers stay predictable.

## Converting

### Observed

- The three *Convert … to New MIDI Track* commands each create a new MIDI track with a MIDI clip.
  **(hypothesis** for the exact track and instrument they add.**)**
- **Dropping an audio clip onto a MIDI track** opens a dialog *Convert Audio To MIDI*: "What type
  of audio material are you converting?" with three choices, *Harmony*, *Melody* and *Drums*,
  each with a small picture of typical notes, and *Cancel*.

### Behaviour

1. **Melody:** one note at a time. Each note gets the pitch heard, its start and its length;
   pitch glides become separate notes. **(hypothesis)**
2. **Harmony:** several notes at once (chords, polyphonic parts). Each note gets its pitch, start
   and length.
3. **Drums:** each hit becomes a note on a fixed key per drum class: kick, snare, hi-hat.
   **(hypothesis** for the classes and keys; the manual lists them.**)**
4. **Velocity** follows the loudness of each note or hit. **(hypothesis)**
5. **Dropping onto a MIDI track:** after a choice, the MIDI clip is written on the track it was
   dropped on, at the drop position, and the audio clip is not placed there. *Cancel* drops
   nothing. **(hypothesis)**

**Loopwerk:** every conversion is a command (`clip.sliceToMidi`, `clip.convertToMidi` with
`material: melody | harmony | drums`), runs off the audio thread, and is one undo step.

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

- Stem separation (*Separate Stems to New Audio Tracks*).
- Converting in real time or while recording.
