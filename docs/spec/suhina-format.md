---
description: The layout of Suhina's own project file (.suhina; .locr from before still read), field by field, with what a reader accepts
read_when: .suhina layout, markers in the file, .locr, project file fields, writing a project file by hand or from an agent, format version, base64 state
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/project-file/src/lib.rs
---

# The .suhina project file

Suhina's own project file ([ADR 0010](../decisions/0010-project-files.md)), version 1. It is
UTF-8 JSON, written pretty-printed. Its parts mirror the project model
([ADR 0001](../decisions/0001-daw-neutral-model.md)); this lists them and what a reader accepts.

```json
{
  "format": "suhina",
  "version": 1,
  "project": {
    "transport": { "tempo_bpm": 120.0, "time_signature": [4, 4] },
    "tracks": [ … ], "channels": [ … ], "contents": [ … ],
    "arrangement": { "lanes": [ … ], "markers": [ … ] },
    "launcher": { "scenes": [ … ], "columns": [ … ] }
  }
}
```

## Rules

- `format` is `"suhina"` and `version` a whole number from 1 to the newest the reader knows;
  anything else is refused. Files saved before 2026-10-13 end in `.locr` and name the format
  `"locr"`: the same format, read the same way. A writer always writes `"suhina"`, also when it
  saves back to a `.locr` path.
- Ids are whole numbers that fit 32 bits. Numbers are finite (a writer writes 0 for one that is
  not). A list or an optional field may be left out or `null`.
- Binary data (`state`, extension `data`) is standard base64.
- `extensions` (on the project, a track, channel, device, content or clip) is a list of
  `{ "format", "key", "data" }`: what an importer kept from its format; it is left out when empty.
- The project must hold together as an imported one must: ids unique, every reference resolves,
  fades fit their clip. A file that does not is refused, naming the first problems.
- A reader refuses a file over 512 MiB.

## Parts

| Part | Fields |
|---|---|
| `transport` | `tempo_bpm` (1–999, default 120); `time_signature` `[numerator, denominator]`, each 1–255 (default `[4, 4]`); `loop` (optional, left out without a loop region): `{ "start", "end", "enabled" }`, beats with `start` from 0 and `end` after it, `enabled` true or false (default false) ([loop region](arrangement-editing.md#loop-region)) |
| track | `id`; `name`; `color` `"#RRGGBB"` or `null`; `notes`, `audio` (what it plays); `channel` (its mixer channel) and `parent` (its group) or `null` |
| channel | `id`; `name`; `role` `regular`, `master`, `effect` (a return) or `submix`; `volume` (linear, not negative, default 1), `pan` (−1 to 1); `mute`, `solo`; `output` (a channel, `null` for the audio output); `sends` `[{ target, level, pre_fader }]`; `devices` |
| device | `name`; `kind` `{ "type": "builtin", "id" }`, `clap`, `vst3` (32 hex digits), `vst2` (a whole number) or `foreign` `{ format, id }`; `enabled` (default true); `state` (base64, plug-ins); `params` `[{ id, value }]` (built-in effects and instruments). A VST3 controller's state is the extension `Vst3ControllerState` |
| content | `id`; `name`; `length` (beats); `lanes`, each `{ target, notes }` or `{ target, audio }` (`target` a track for a shared pattern, else `null`) |
| note | `time`, `duration` (beats); `key` 0–127; `velocity`, `release_velocity` 0–1; `channel` 0–15 |
| audio | `file` (relative to the project file, `/` between folders); `absolute_file`; `sample_rate`, `channels`, `duration_seconds` (if known); `warped`; `stretch` `repitch`, `rhythmic`, `tonal`, `texture`, `smooth` or `high_quality`; `warp_markers` `[{ time, audio_time }]`; `gain` (linear); `transpose` (semitones); `stream`: which audio track of a file with several plays (a Stems file: 0 the master, 1 and up the stems; missing or `null` for an ordinary file) |
| arrangement | `lanes`; `markers` `[{ beat, name }]`: named positions on the timeline, `beat` a number from 0 on; read in any order and kept in beat order; left out when there are none ([arrangement editing](arrangement-editing.md#markers)) |
| arrangement lane | `name`; `track` or `null`; `clips` |
| clip | `content`; `start`, `length`, `offset` (beats); `loop` `[start, end]` or `null`; `enabled`; `fade_in`, `fade_out` (beats); `fade_in_shape`, `fade_out_shape` (`linear`, `equal_power`, `fast`, `slow` or `s_curve`; default `linear`) and `fade_in_curve`, `fade_out_curve` (−1 to 1, default 0), see [arrangement-editing](arrangement-editing.md#fades) |
| scene | `name`; `tempo_bpm` or `null` |
| launcher column | `track`; `slots` `[{ clip, has_stop }]`, one per scene |

## Audio paths

A project saved to another folder gets each audio path written relative to its new file (with
`..` where needed, on the same drive) and the file's absolute path; opening it looks for the
relative path first, then the absolute one. The audio is not copied.
