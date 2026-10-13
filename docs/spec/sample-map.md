---
description: How the sample map behaves: samples laid out by how they sound, coloured by kind, as a cloud or a grid, previewed by pointing, dragged into the project; NKS preset previews in the same map
read_when: sample map, similarity, sound map, browse by sound, t-SNE, UMAP, point cloud, grid layout, similar samples, kick snare hat categories, audio features, MFCC, NKS, preset previews, soundmap commands
type: spec
status: draft
last_verified: 2026-10-13
covers:
  - crates/soundmap/**
  - crates/session/src/soundmap.rs
  - crates/ui/src/sample_map.rs
  - crates/ui/src/sample_map_panel.rs
---

# Sample map

A view of the browser that places samples on a plane by how they sound: samples that sound
alike lie close together, kicks gather in one region and hats in another. Pointing at a sample
plays it, so a library is explored by moving the mouse instead of reading names. Instrument
presets that come with audio previews (NKS) take part in the same map.

- **Sources:** the maintainer's request (2026-10-12) with two screenshots of commercial drum
  plug-ins that have such a view; public papers and open-source projects listed under
  *Prior art*. The screenshots were used for structure and behaviour only (what the view shows
  and how it reacts); the look follows [`docs/design/brief.md`](../design/brief.md). Nothing
  was run, measured or taken from those products.
- **Builds on:** the browser ([`browser.md`](browser.md)), its file library
  ([ADR 0012](../decisions/0012-file-library.md)) and previewing
  ([ADR 0011](../decisions/0011-preview-and-added-audio.md)).
- **Architecture:** [ADR 0013](../decisions/0013-sample-map-analysis.md): the analysis crate,
  the features, the layout methods and the cache.
- **Status:** built, except NKS presets: the analysis, layout, grid, kinds and caches
  (`crates/soundmap`), the `soundmap.*` commands (`crates/session/src/soundmap.rs`), the map
  view (`crates/ui/src/sample_map.rs`) and its place in the browser and the main area
  (`crates/ui/src/sample_map_panel.rs`). NKS presets wait for the format to be checked on our
  own files ([`nks-format.md`](nks-format.md) › To observe). Statements marked **(to decide)**
  are open.

## What was observed

Two existing products show the idea in two forms. In our own words:

1. **A free cloud.** Every sample is a small dot; the dots form a cloud in which similar
   sounds are near each other. The dot's colour says what kind of sound it is (kick, snare and
   clap, hats and so on); colours blend where the kinds meet. The samples the instrument is
   using are drawn as larger dots in the colour of the slot that uses them. A search field sits
   above the cloud; zoom in, zoom out and fit buttons at a corner; a small overview of the cloud
   beside the slots.
2. **A grid.** The same idea with every dot snapped to a cell of a grid, one sample per cell,
   so no dot hides another and each is easy to hit. The name of each kind (kick, crash, closed
   hi-hat, tom, other) is written in large letters over its region. The map sits beside a file
   browser, drum pads and a step sequencer.

Both play a sample as soon as the pointer is over it.

## Terms

- **Map:** the plane with one **point** per item. An item is a sample file from the library or
  an NKS preset with a preview (see *NKS presets*).
- **Scope:** the items a map holds: those of the place selected in the browser (a folder, Pinned,
  a Recent group), or of every folder when no place is selected.
- **Features:** numbers computed from an item's audio that describe its sound
  ([ADR 0013](../decisions/0013-sample-map-analysis.md) lists them). Two items are **similar**
  when their features are close.
- **Layout:** the position of every point, computed from the features of the scope.
- **Kind:** what sort of sound an item is (kick, snare, hat…), from its name or, failing that,
  guessed from its features.

## Where the map is shown

- **In the browser.** A **List | Map** switch above the results changes the results area into
  the map. The sidebar, the category filters, search, tags and the preview strip stay as they
  are. The browser can be widened to 600 px.
- **Large.** The **Large** button at the map's corner (and View › Sample map,
  **Ctrl+Shift+M**) shows the map in the main area in place of the Session or Arrangement view,
  at full size; **Close**, Ctrl+Shift+M, Escape (when Show similar is not on) or the view
  switches go back. The browser stays beside it and shows its list meanwhile. While something is
  dragged from the large map, the main view shows instead, so the drop can land on a track.

The map holds the samples of the browser's place, whichever category filter is chosen (the
filters choose what the list shows).

**Suhina:** analysis starts with the desktop app and the MCP server, as indexing does; tests
and the command line never analyse a user's folders unasked.

## Analysis

Before an item can be placed, its audio is analysed. This happens in the background:

- When a folder is added or its index changes (new, changed or removed files), its new and
  changed samples are queued; removed ones leave the cache.
- Analysis uses at most half of the computer's cores, never the audio thread, and the
  results are kept on disk, so a library is analysed once.
- A sample is analysed from its first 4 seconds. Samples longer than the **map length limit**
  (Settings › Interface › Browser › Map: longest sample: 5, 10, 30 or 60 s, 10 by default;
  `soundmap.layout` takes 1–60) are not shown: the map is for one-shots and short loops. The
  length is kept with the analysis, so changing the limit needs no new analysis.
- The map's top bar says what it holds: "1 240 samples, analysing, 6 760 to go, 12 too long,
  3 could not be read". While the analysis goes on, the layout is computed again at most every
  2 seconds, on a thread of its own ("arranging…"), so the window never waits for it.
- A file that cannot be decoded is left out and counted; `soundmap.info` gives the reason.

## Layout

- **Cloud** (the default): points where the layout puts them. Similar items are close; distances
  between far-apart regions mean little.
- **Grid:** the same layout snapped to a grid of square cells, one item per cell, keeping
  neighbours together as far as possible. The grid is as close to the map's aspect ratio as the
  item count allows, and is made again when the map's shape changes; `soundmap.layout` takes the
  aspect ratio as `aspect`.
- A **Cloud | Grid** switch at the top of the map chooses; the choice is kept.
- The same scope with the same items always opens with the same layout (layouts are kept),
  so a user can learn where things are. Adding or changing files lays the scope out again
  **(to decide:** whether new items are placed into the existing layout instead, until a
  Re-layout button is pressed**)**.
- A large scope takes a while to lay out the first time (about 5 seconds for 10 000 items);
  meanwhile the map shows "Arranging…" over the previous layout or an empty map. A scope with
  more than 100 000 items shows the first 100 000 and says so.

## Kinds and colours

Every item has a kind. The kinds and the words in names that give them:

| Kind | Words (whole words in the file name or its folders, any case) |
|---|---|
| Kick | kick, kik, kck, bd, bassdrum, bass drum |
| Snare | snare, snr, sd |
| Clap | clap, clp, cp, snap |
| Closed hat | chh, closedhat, closed hat, hhc, hat closed, closed hihat, hihat closed, hh closed |
| Open hat | ohh, openhat, open hat, hho, hat open, open hihat, hihat open, hh open |
| Hat | hat, hihat, hh |
| Rim | rim, rimshot, rs, stick |
| Tom | tom, toms, floor tom |
| Crash | crash |
| Ride | ride |
| Cymbal | cymbal, cym, china, splash |
| Percussion | perc, shaker, shk, tamb, tambourine, conga, bongo, cowbell, clave, block, triangle |
| Bass | 808, bass, sub |
| Tonal | stab, chord, pluck, keys, synth, lead, pad |
| Vocal | vox, vocal, voice, chant |
| Effect | fx, sfx, riser, impact, sweep, noise |
| Other | anything else |

- Names are split into words at spaces, punctuation, changes of case and between letters and
  digits (`KickHard_02` → kick, hard, 02); a word with a trailing *s* also counts as the word
  without it (`Kicks`, `Hats`) when what is left has at least three letters and the word is not
  itself in the table. Two words in a row (`closed hat`, `bass drum`) count before one word
  alone. The file name is read first, then its folders from the
  nearest up to the library folder; the first word that names a kind wins, and a more specific
  kind (Closed hat) wins over a general one (Hat) in the same name.
- **Guessed kinds.** An item whose name gives no kind is given the kind most of its 7 nearest
  neighbours (by features) have, when at least 4 of them agree; otherwise Other. A guessed
  kind is drawn a little paler, and the item's tooltip says "guessed".
- Each kind has a colour from Suhina's own palette (the design brief's track colours, extended),
  chosen so that neighbouring kinds (kick and bass, snare and clap, the hats) are told apart.
- **Labels.** The name of each kind with at least 8 items in view is written over its region, at
  the item of that kind closest to the kind's centre, in the map's label style. Labels fade out
  when zoomed in far enough to read point names.
- **Legend:** a row under the map with each kind, its colour and its count. Clicking a kind dims
  all others (Ctrl+click: several); clicking it again stops.

## Points

- A point is a dot (cloud) or a filled cell (grid) in its kind's colour.
- **In use:** items used by a clip of the active project are drawn larger with a ring.
- **Selected:** items selected in the browser have a light outline; the map and the list share
  one selection.
- **Previewed:** the item last previewed, and the one under the pointer, have an accent ring.
- **Similar:** while Show similar is on, the similar items have an accent ring and the others
  are dimmed.
- **Dimmed:** items that do not match the search text, the tag filter or the legend are drawn
  faint but stay in place, so the map does not jump while typing.
- **Tooltip** over a point: the file name, the kind ("guessed" when it is), and its folder.

## Pointing, clicking and moving

| Input | Does |
|---|---|
| Pointer over a point | Highlights it and **previews it** (Settings › Interface › Browser › Map: preview on hover, on by default). Moving across points plays each in turn; a new one stops the one before. A point is played only after the pointer has rested on it for 40 ms, so a fast sweep does not play everything it crosses. |
| Pointer resting on a point | Its tooltip shows the name, the kind, the folder and, after 0.15 s, the file's waveform, read in the background ([waveforms](waveforms.md)). |
| Click a point | Selects it and previews it (also with preview on hover off) as the button goes down, as in the list. |
| Ctrl+click, Shift+drag on empty space | Adds to or removes from the selection; Shift+drag draws a rectangle and selects what is in it. |
| Drag a point | Drags the selection into the project, exactly as from the list (drop targets, labels, Ctrl for one track each): [`browser.md`](browser.md) › Dragging into the project. |
| Double-click a point | Adds it to the selected track, as in the list. |
| Drag empty space, middle-drag | Moves the view. |
| Mouse wheel | Zooms around the pointer (Ctrl+wheel is not needed). |
| Double-click empty space, the Fit button | Fits all points in view. |
| Arrow keys | Moves the selection to the nearest point in that direction and previews it; with Show similar on, Up and Down step through the similar list instead. |
| Right-click a point | Menu: **Show similar**, Pin or Unpin, Tag ▸, Show in folder (switches to the list with the item selected). |

**Show similar** rings the 16 items nearest to the clicked one by features (not by position on
the map, which distorts distances), dims the rest and lists them in order in a strip at the map's
side, each with a play button; Escape or clicking empty space ends it. Arrow keys Up and Down then
step through the list, starting from the item it was shown for, and preview each; Left and Right
still move to the nearest point on the map.

The previews are the browser's own (`preview.play`): same volume, same output, stopped by the
same things.

## NKS presets

NKS is a format in which instrument presets (`.nksf`) and effect presets (`.nksfx`) are kept as
files that name the plug-in they belong to, carry its state and metadata (name, author, bank,
sound types), and often have a short audio **preview** recorded from the preset: in a hidden
`.previews` folder in the same folder, named after the whole preset file (`Bass.nksf` →
`.previews/Bass.nksf.ogg`). The format is described from public sources and our own files in
[`nks-format.md`](nks-format.md).

- **Listed in the browser:** `.nksf` and `.nksfx` files in an added folder are presets
  (category Presets), with the plug-in they need, the bank and the types from their metadata.
  Folders are found the way the browser finds any folder: the user adds them (the usual place
  for user and third-party content is `Documents\Native Instruments\User Content`). Databases
  and registry entries of other programs are not read.
- **Naming:** the browser and the docs say "Native Instruments preset files (.nksf)" where a
  name is needed; Suhina does not use the NKS logo or call itself NKS-compatible.
- **In the map:** a preset with a preview is an item of the map; its features come from the
  preview. Its kind comes from its sound type metadata when there is one, then from its name as
  above. Presets are drawn as rings rather than dots, so they are told from samples.
- **Previewing** plays the preview file, as a sample. A preset without a preview is listed but
  not in the map.
- **Dragging** a preset into the project adds the plug-in it names with the preset's state, as a
  plug-in from the browser: on a track's chain, or a new track in the empty area. When the plug-in
  is not installed, or is not a format Suhina hosts, the drop is refused with a message naming
  the plug-in.

How much of this works with a given library is open until the format is checked on real files;
see *To find out*.

## Commands

Everything the map does is a command (AGENTS.md rule 6):

| Command | Params | Does |
|---|---|---|
| `soundmap.status` | | analysis progress per folder: analysed, queued, failed, skipped as too long |
| `soundmap.analyse` | `folder?` | queues a folder (or all) for analysis again, ignoring the cache |
| `soundmap.layout` | `place?`, `text?`, `tags?`, `grid?`, `aspect?`, `maxSeconds?` | the scope's items with their position (0–1 in both axes), kind, whether the kind was guessed and whether they match `text`/`tags`; leaves out samples longer than `maxSeconds` (1–60, default 10); with `grid`, each item's cell in a grid whose width over height is close to `aspect` (0.1–10, default 1); computes the layout when it is not cached |
| `soundmap.similar` | `item`, `count?`, `place?` | the `count` (default 16, at most 100) items most similar to `item` in its scope (`place`), nearest first, with their distance |
| `soundmap.info` | `item` | an item's kind (and why: the word, or guessed), its features and when it was analysed |

Over MCP, `item`, `place` and `folder` paths must be inside the server's read roots
([`mcp.md`](../mcp.md)).

## Limits

| What | Limit | Beyond it |
|---|---|---|
| Sample length in the map | 10 s by default, 1–60 s | not analysed, not shown |
| Audio analysed per item | first 4 s | the rest is not read |
| A file to analyse | 256 MiB | skipped with a reason |
| Items in one map | 100 000 | the rest left out; the map says so |
| Grid part assigned exactly | 256 items | larger grids split first (ADR 0013) |
| `count` in `soundmap.similar` | 100 | cut |
| Feature cache per folder | 64 MiB | not read; the folder is analysed again |

## Prior art

Studied for the method; no code is taken from them unless its licence allows and ADR 0013 says
so.

- **AudioStellar** (GPL-3.0-or-later, C++): a folder of samples analysed (spectrum, MFCC),
  reduced (PCA, t-SNE or UMAP), clustered, shown as a coloured cloud played by pointing. The
  nearest open equivalent of this view.
- **FluCoMa** (BSD-3-Clause, C++): corpus maps from MFCC, spectral shape, loudness and pitch
  statistics, standardised, reduced with UMAP or MDS, a k-d tree for neighbours, and a grid
  object that snaps a 2D layout to a grid.
- **Freesound Explorer** (MIT, JavaScript): t-SNE maps of sounds by spectral descriptors with
  hover to play.
- **Infinite Drum Machine** (Apache-2.0, JavaScript) and its analysis notebooks (MIT): t-SNE of
  small spectrograms of one-shots.
- **RasterFairy** (BSD-3-Clause, Python): snapping a 2D cloud to a grid by recursive splitting.
- Papers: Pampalk, Hlavac and Herrera, *Hierarchical Organization and Visualization of Drum
  Sample Libraries* (DAFx 2004); Turquois et al., *Exploring the Benefits of 2D Visualizations
  for Drum Samples Retrieval* (CHIIR 2016); Shier et al., *Manifold Learning Methods for
  Visualization and Browsing of Drum Machine Samples* (JAES 2021); Richan and Rouat, *A
  proposal and evaluation of new timbre visualisation methods for audio sample browsers* (2020);
  Fried et al., *IsoMatch* (2015) and Barthel et al., *Improved Evaluation and Generation of
  Grid Layouts using Distance Preservation Quality and Linear Assignment Sorting* (2022) for
  grids.

Repositories without a licence (ml4a's audio t-SNE viewer, the code of Shier et al., IsoMatch)
gave ideas only, through their papers and descriptions; no code was copied, ported or
translated from them (CONTRIBUTING.md › Clean-room).

## To find out

- The NKS questions listed in [`nks-format.md`](nks-format.md) › To observe, above all whether a
  preset's state loads into the VST3 version of its plug-in.
- Whether the guessed kinds are good enough with nearest neighbours, or a small classifier
  trained on openly licensed one-shots (CC-BY or CC0 only) is needed.
- How the layout feels with real libraries of 1 000, 10 000 and 100 000 samples: perplexity,
  the 4-second window and the feature weights are set by trying them.

## Not decided yet

- Loops: tempo-synced preview of loops in the map, and a layout for longer files.
- Placing new items into an existing layout instead of computing it again.
- Building a drum kit from a selection on the map.
