# Attribution

Every non-code asset in this repository, with its author, source and license. Loopwerk's own
code and original assets are GPL-3.0-or-later (see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE)).
When you add an asset (sample, impulse response, preset, font, icon, image), add a row here in the
same change. Third-party assets must have a GPLv3-compatible license (for example MIT, BSD, ISC,
Apache-2.0, OFL, CC0, CC BY) and keep their license file next to them.

## Bundled in the app

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `crates/ui/fonts/encode-sans/EncodeSansSemiExpanded-Regular.ttf` | Encode Sans Semi Expanded Regular (the UI typeface) | The Encode Project Authors (Pablo Impallari, Andres Torresi, Jacques Le Bailly) | [google/fonts](https://github.com/google/fonts/tree/main/ofl/encodesanssemiexpanded) | OFL-1.1 (`crates/ui/fonts/encode-sans/OFL.txt`) |
| `crates/ui/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf` | IBM Plex Mono Regular (numbers, times, positions) | IBM Corp. | [google/fonts](https://github.com/google/fonts/tree/main/ofl/ibmplexmono) | OFL-1.1 (`crates/ui/fonts/ibm-plex-mono/OFL.txt`) |
| `crates/ui/themes/dracula.json` | Dracula colour palette, mapped onto Loopwerk's tokens | Dracula Theme | [dracula/dracula-theme](https://github.com/dracula/dracula-theme) | MIT (`crates/ui/themes/licenses/dracula.txt`) |
| `crates/ui/themes/nord.json` | Nord colour palette, mapped onto Loopwerk's tokens | Sven Greb | [nordtheme/nord](https://github.com/nordtheme/nord) | MIT (`crates/ui/themes/licenses/nord.txt`) |
| `crates/ui/themes/catppuccin-mocha.json`, `catppuccin-latte.json` | Catppuccin Mocha and Latte colour palettes, mapped onto Loopwerk's tokens | Catppuccin | [catppuccin/palette](https://github.com/catppuccin/palette) | MIT (`crates/ui/themes/licenses/catppuccin.txt`) |
| `crates/ui/themes/solarized-dark.json` | Solarized colour palette (dark), mapped onto Loopwerk's tokens | Ethan Schoonover | [altercation/solarized](https://github.com/altercation/solarized) | MIT (`crates/ui/themes/licenses/solarized.txt`) |
| `crates/ui/themes/editor-dark.json`, `loopwerk-light.json`, `high-contrast.json` | Loopwerk's own themes | Loopwerk contributors | Original work | GPL-3.0-or-later |

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

## Vendored code

Third-party source kept unchanged in `third_party/`, built into Loopwerk only with a Cargo
feature.

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `third_party/asiosdk` | Steinberg ASIO SDK 2.3.4, host part (`asio` feature) | Steinberg Media Technologies GmbH | https://www.steinberg.net/asiosdk | GPL-3.0 (`third_party/asiosdk/LICENSE.txt`; see `VENDORED.md`) |

## Test data in the repository

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `devices/fm-synth/tests/golden/scenarios.txt` | FM synth test voices and notes | Loopwerk contributors | Original work | GPL-3.0-or-later |
| `devices/fm-synth/tests/golden/*.f32` | Reference renders of those scenarios | Loopwerk contributors | Rendered with the MSFA engine by `tests/reference/harness.cpp` | GPL-3.0-or-later |
| `crates/engine/tests/golden/*.f32` | Reference renders of clip fades | Loopwerk contributors | Rendered by Loopwerk's engine (`src/fade_tests.rs`) | GPL-3.0-or-later |

## Test data (not committed, not shipped)

`corpus/` is gitignored. Test projects are authored by Loopwerk contributors (see
[`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room) and are GPL-3.0-or-later. Never add projects,
samples, presets or Packs that ship with a commercial DAW.

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| _(none yet)_ | | | | |
