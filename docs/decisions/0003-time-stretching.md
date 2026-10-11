# 0003: Time-stretching and stretch modes

- **Status:** accepted
- **Date:** 2026-10-11
- **Task:** LOOP-3

## Context

Warped audio clips map beats to times in their file through warp markers. Playing them by
reading the file faster or slower ("re-pitch") keeps the timing right but changes the pitch with
the speed. In a real project (a lead vocal warped with 43 markers, segment speeds 0.486–0.516
s/beat against 0.5) this made the vocal wobble by about ±0.5 semitones at every marker.

DAWs solve this with time-stretching that keeps the pitch, done **while playing**, so the tempo
can change at any moment. They offer several algorithms because no single one suits everything:
drums need their transients kept, voices their pitch and formants, full mixes a heavier,
higher-quality algorithm. Ableton Live calls these warp modes (Beats, Tones, Texture, Re-Pitch,
Complex, Complex Pro); our own Live Set had `WarpMode="4"`, which Live showed as Complex. Live
also keeps a per-file analysis cache (`.asd`) so opening a set does not redo that work.

GPL-compatible stretchers usable from Rust (checked 2026-10-11):

| Library | Licence | Built from | Notes |
|---|---|---|---|
| timestretch (robmorgan/timestretch-rs) | MIT | pure Rust | Real-time engine: no allocation on the audio path, `forbid(unsafe_code)`, tempo changes at exact output frames, gapless loop re-anchoring. Quality gated in CI against Rubber Band. DJ-oriented: keylock profile within ±20 %, full-range profile 0.25×–2×. |
| Rubber Band | GPL-2.0-or-later | C++ | The reference for quality at any ratio (R3 engine). Needs a C++ compiler and FFI. |
| Signalsmith Stretch | MIT | C++ | High quality; the Rust wrapper needs bindgen and libclang. |
| Bungee | MPL-2.0 | C++ | Small, little-used Rust bindings. |

## Decision

1. **Stretch modes are part of the model.** Every audio region has a DAW-neutral
   `StretchMode`: `Repitch`, `Rhythmic`, `Tonal`, `Texture`, `Smooth` and `HighQuality`.
   Format importers map their own codes to these; Live's `WarpMode` 4 (Complex) maps to
   `Smooth`. Codes not yet observed in our corpus map to `Smooth` with a warning.
2. **Stretching happens in real time, in the engine.** Every warped clip placement gets a
   stretch voice when the renderer is built: a timestretch engine plus a playlist that says
   which part of the file plays when and how fast (warp marker segments and loop jumps, at the
   project tempo). In `process` the voice copies source frames into the stretcher's ring,
   schedules rate changes at exact frames and runs the stretcher; it never allocates (AGENTS.md
   rule 3). Offline rendering runs the same voices (rule 4), and the output does not depend on
   the block size. A tempo change only means a new playlist, not new audio.
   - The stretcher runs at the **file's** sample rate, so the pitch is kept against the file's
     own pitch; its output is read at the project rate with linear interpolation.
   - Audio that already plays at its own speed everywhere (within 0.01 %) skips the stretcher
     and is read directly.
3. **Loading a project decodes and analyses.** `loopwerk-media` decodes every file on all cores
   and computes the stretcher's pre-analysis (onsets, beat grid) once per file that a warped clip
   plays. A disk cache for the analysis, as Live has, comes later.
4. **Modes choose backends behind one interface.** Now:

   | Mode | Backend |
   |---|---|
   | `Repitch` | none (read the file faster or slower) |
   | `Rhythmic`, `Tonal`, `Texture`, `Smooth`, `HighQuality` | timestretch: keylock profile while every segment stays within ±20 % of the file's speed (it keeps the pitch there, bass included beyond about ±2 %, at a low and even cost), the full-range profile beyond that (0.25×–2×); outside that range the clip is re-pitched |

   The modes all map to the same backend for now; they are kept in the model so projects keep
   the user's intent and later backends can tell them apart.
5. **Rubber Band comes later, as an isolated FFI crate.** Calling C++ needs `unsafe`, which the
   workspace forbids. When we add Rubber Band for `HighQuality`, it gets one crate
   (`crates/rubberband`) with `unsafe` allowed only there, a `SAFETY:` comment on every block
   and a safe interface for the rest of the code, as PhotoCraft does for its tablet crate. That
   exception is recorded in AGENTS.md when it is made. Owner decisions 2026-10-11: timestretch
   first, Rubber Band afterwards; real-time stretching now rather than pre-rendering.

## Measurements (2026-10-11, the warped-vocal project: 80 s, 58 warped clips, 25 files)

| | Pre-render at load (first try) | Real time, full-range profile only | Real time, as decided |
|---|---|---|---|
| Opening the set | 10.6–11.8 s | 3.1 s | 3.1 s |
| Average load on one core while playing | – | 52 % | 20 % |
| Worst 512-frame block (budget 10.7 ms) | – | 42 ms | 8.6 ms |
| Tempo change | stretch everything again | new playlists only | new playlists only |

## Consequences

- Warped clips keep their pitch while playing, and their timing still comes from the warp
  markers.
- CPU use grows with the number of warped clips playing at once. The worst block is close to its
  budget in a heavy project, so stretch voices should next render a little ahead on a worker
  thread (lock-free rings to the audio thread) to absorb peaks.
- Every clip placement owns a stretcher, so memory grows with the number of warped placements;
  pooling voices across clips that do not overlap is a later optimisation.
- Our own algorithm choice, not Live's: modes with the same name in another DAW will not sound
  identical. The modes describe intent, not a copy.
- Comparing backends will be a mode switch on the same clip once Rubber Band is in.
