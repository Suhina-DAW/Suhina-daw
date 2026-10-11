---
description: Real-time time-stretching of warped audio, stretch modes and backends, with measurements
read_when: warp, time-stretch, stretch mode, keylock, timestretch, Rubber Band, audio crackles, CPU load
type: decision
status: current
last_verified: 2026-10-11
covers:
  - crates/engine/src/stretch.rs
  - crates/media/src/lib.rs
---

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
| timestretch (robmorgan/timestretch-rs) | MIT | pure Rust | Real-time engine: no allocation on the audio path, `forbid(unsafe_code)`, tempo changes at exact output frames, gapless loop re-anchoring. Quality gated in CI against Rubber Band. DJ-oriented: keylock profile within ±20 %, full-range profile 0.25×–2× as documented (we measured it holding time only from 0.5×). |
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
   - **Transients stay on the grid.** A stretcher places transients exactly only right after it
     starts; later they drift by up to half its grain (keylock: up to ~9 ms early when slowing
     down), so a stretched drum loop flams against unstretched ones and against the vocals. Each
     voice therefore has two stretchers: shortly before every strong onset of the file (from the
     pre-analysis) the idle one is restarted at exactly the right source frame and the voice
     crossfades into it (`ANCHOR_LEAD`, `ANCHOR_FADE`). For the keylock the crossfade ends 12 ms
     before the onset, more than the old stretcher can have drifted; the phase vocoder smears an
     onset some 20 ms ahead of itself, so its restart comes closer to the onset and the crossfade
     hides part of the smear. The restart's start-up is spread over the
     frames before it, so voices whose onsets fall on the same beat do not all pay for it in one
     audio block, and the first start-up happens when the renderer is built.
   - The full-range profile reports no pipeline delay, but its phase vocoder slews its rate from
     1× after a reset and its window delay is missing from its source-position map: measured,
     output lands about `31 ms × (1 − 1/rate)` late. A restart feeds a preroll, runs until the
     stretcher reports the start point playing and corrects by that measured amount. The keylock
     has a smaller delay of the same form, about `32 ms × (1 − 1/rate)`, corrected the same way.
   - The phase vocoder clamps its ratio to 0.5–2, so the full-range profile covers 0.5×–2×
     (rates below 0.5× drifted by 130–320 ms); slower clips are re-pitched.
   - At the end of the playlist the file carries on into the stretcher, so a stretcher that
     looks ahead (the phase vocoder, 2048 frames) can finish the clip's last frames instead of
     going silent ~50 ms early.
   - **Transpose** (`AudioRegion::transpose`) happens inside the same voice: the stretcher runs
     at `rate / p` (p = 2^(semitones/12)), making the audio p times longer at its own pitch, and
     its output is read p times faster, which raises the pitch by p and restores the length. In
     the stretcher's own time every playlist piece is p times longer and slower, so rate changes,
     restarts and lane starts work unchanged. A transpose that takes a segment outside 0.5×–2×
     re-pitches the clip and ignores the transpose.
3. **Loading a project decodes and analyses.** `loopwerk-media` decodes every file on all cores
   and computes the stretcher's pre-analysis (onsets, beat grid) once per file that a warped clip
   plays. A disk cache for the analysis, as Live has, comes later.
4. **Modes choose backends behind one interface.** Now:

   | Mode | Backend |
   |---|---|
   | `Repitch` | none (read the file faster or slower) |
   | `Rhythmic`, `Tonal`, `Texture`, `Smooth`, `HighQuality` | timestretch: keylock profile while every segment stays within ±20 % of the file's speed (it keeps the pitch there, bass included beyond about ±2 %, at a low and even cost), the full-range profile beyond that (0.5×–2×); outside that range the clip is re-pitched |

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

Transient timing, measured with clicks every 0.5 s (`crates/engine/src/stretch.rs` tests; error
from where the warp markers put each click):

| | Before restarts | With restarts |
|---|---|---|
| Keylock 0.95× | up to 9.4 ms early | within 1.7 ms |
| Keylock with changing segment rates | up to 9.6 ms | within 2.4 ms |
| Full range 0.7× / 1.5× | 19 ms early / 4 ms late on average, up to 30 ms | 5 ms early / 1 ms early on average |

On the same project the restarts cost about 3 % more CPU; the worst 512-frame block measured
8.3–10.3 ms against 8.0–8.7 ms before (budget 10.7 ms). Rendering ahead on a worker thread is
what gives the real margin.

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
