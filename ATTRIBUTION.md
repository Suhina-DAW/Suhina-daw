# Attribution

Every non-code asset in this repository, with its author, source and license. Loopwerk's own
code and original assets are GPL-3.0-or-later (see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE)).
When you add an asset (sample, impulse response, preset, font, icon, image), add a row here in the
same change. Third-party assets must have a GPLv3-compatible license (for example MIT, BSD, ISC,
Apache-2.0, OFL, CC0, CC BY) and keep their license file next to them.

## Bundled in the app

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| _(none yet)_ | | | | |

## Documentation

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `docs/design/reference/session-minimal-v3.png`, `arrangement-minimal-v3.png` | Loopwerk Session and Arrangement design mockups (minimal v3) | Loopwerk contributors | Original work, exported from the project's design canvas | GPL-3.0-or-later |

## Ported code

Code translated from other open-source projects; each file keeps its notices and says what
changed. The full list with copyright lines is in [`NOTICE`](NOTICE).

| Path | Ported from | Author | Source | License |
|---|---|---|---|---|
| `crates/stretch` | Signalsmith Stretch, Signalsmith Linear (STFT) | Geraint Luff / Signalsmith Audio | https://github.com/Signalsmith-Audio/signalsmith-stretch, https://github.com/Signalsmith-Audio/linear | MIT |
| `devices/fm-synth/src/{tables,env,lfo,core,voice}.rs` | MSFA engine as found in Dexed | Google Inc., Pascal Gauthier, Jean Pierre Cimalando | https://github.com/asb2m10/dexed (`Source/msfa`) | Apache-2.0 |

## Test data in the repository

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `devices/fm-synth/tests/golden/scenarios.txt` | FM synth test voices and notes | Loopwerk contributors | Original work | GPL-3.0-or-later |
| `devices/fm-synth/tests/golden/*.f32` | Reference renders of those scenarios | Loopwerk contributors | Rendered with the MSFA engine by `tests/reference/harness.cpp` | GPL-3.0-or-later |

## Test data (not committed, not shipped)

`corpus/` is gitignored. Test projects are authored by Loopwerk contributors (see
[`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room) and are GPL-3.0-or-later. Never add projects,
samples, presets or Packs that ship with a commercial DAW.

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| _(none yet)_ | | | | |
