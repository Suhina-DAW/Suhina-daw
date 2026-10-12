---
description: How the sample map analyses samples (features), lays them out (PCA and t-SNE, grid assignment), finds similar ones (k-d tree), caches the results, and which crates it uses
read_when: sample map, soundmap crate, audio features, MFCC, spectral centroid, t-SNE, bhtsne, PCA, UMAP, grid layout, linear assignment, lapjv, RasterFairy, nearest neighbours, kiddo, feature cache
type: decision
status: current
last_verified: 2026-10-12
---

# 0013: The sample map: features, layout and cache

- **Status:** accepted
- **Date:** 2026-10-12

## Context

The sample map ([spec](../spec/sample-map.md)) places a library's samples on a plane by how
they sound. That needs, for every sample, a short list of numbers describing its sound
(features); a way to turn tens of thousands of such lists into 2D positions where near means
similar (dimensionality reduction); a grid version of those positions; and a fast way to find
an item's nearest neighbours. Libraries hold up to hundreds of thousands of files, so the
analysis must be cached and incremental, and it must never disturb playback.

The method is well established in open research and software (the spec's *Prior art*):
per-frame spectral features summarised by their mean and spread, standardised, reduced with
t-SNE or UMAP. What remains is choosing the features, the methods and the crates.

## Decision

1. **A crate of its own, `loopwerk-soundmap` (layer 2, beside `library`)**, depends on `media`
   (decoding) and `library` (which files exist, `Library::files`, `Library::revision`). It holds
   the analysis queue and threads, the feature cache, the layouts and the neighbour index. The
   `soundmap.*` commands run on it, held by `Projects` like the library. Nothing in it touches
   the audio thread.
2. **Features, version 1** (44 numbers per item), from a mono sum of the first 4 seconds,
   resampled to 22 050 Hz, in frames of 1 024 samples with a hop of 256, with a Hann window:
   - 13 MFCCs (40 mel bands, the zeroth coefficient left out): mean and standard deviation
     over the frames whose level is within 40 dB of the loudest (26);
   - spectral centroid, spread, flatness, roll-off (85 %) and flux: mean and standard deviation
     over the same frames (10);
   - zero-crossing rate: mean and standard deviation (2);
   - the level envelope: attack time (start to peak), decay time (peak to −20 dB), effective
     length (above −40 dB) and crest factor (4);
   - pitch: the median fundamental of the voiced frames in semitones and the share of voiced
     frames, by a YIN-style estimate (2).

   Times are in log seconds, so short sounds are told apart. Every feature has a golden test
   against values computed once with librosa (ISC) and written into the test. The feature
   list has a version; a cache entry of another version is analysed again.
3. **Distances** are Euclidean over features standardised across the scope (each one to mean 0,
   deviation 1), with the four groups (MFCC, spectral shape, envelope, pitch) weighted equally
   regardless of how many numbers they hold.
4. **Layout:** PCA down to 20 dimensions, then Barnes-Hut t-SNE (perplexity 30, or a third of
   the item count when that is smaller; 1 000 iterations), seeded from a hash of the scope's
   item list so the same scope gives the same map. The result is scaled to fit 0–1. Scopes
   over 20 000 items run t-SNE on a seeded sample of 20 000; each other item goes at the mean
   position of its 5 nearest neighbours (by features) in the sample. Layouts are computed on
   the analysis threads and cached in memory and on disk by scope hash.
5. **Grid:** the cloud's positions are assigned to grid cells by linear assignment (squared
   distance from point to cell centre, Jonker-Volgenant) for up to 2 500 items. Larger grids
   are made by recursive splitting (the RasterFairy method): the cloud and the grid are both
   cut in two along the longer axis, with the same item count on each side, until the parts
   are small enough to assign exactly.
6. **Neighbours:** a k-d tree over the standardised features answers "similar items"; a 2D
   k-d tree over the layout answers "which point is under the pointer" and the arrow keys.
7. **Kinds** come from words in names (the spec's table). An item without one takes the kind
   most of its 7 nearest neighbours have, when 4 or more agree. No trained model in this
   version.
8. **Cache** in `<settings>/library/features-<hash>.bin` beside the library's index, one file
   per folder: a version header, then per file its path below the folder, size, modification
   time, feature version and features (32-bit floats). An entry whose size or time no longer
   matches the library's index is analysed again. Written to a file beside it and moved over
   it, as the index is. A file that does not parse is ignored and the folder analysed again.
9. **Threads:** analysis runs on a pool of half the cores (at least one), started by
   `Library::start`'s caller as indexing is, so tests and the CLI never analyse a user's
   folders unasked. A file larger than 256 MiB or longer than the map's length limit is
   skipped without decoding past its header.
10. **Crates**, all GPL-compatible: `realfft` (MIT) for the transforms; `bhtsne` (MIT) for
    t-SNE; `petal-decomposition` (Apache-2.0) for PCA; `kiddo` (MIT OR Apache-2.0) for k-d
    trees; `lapjv` (MIT) for assignment. MFCC, the spectral features, YIN and the recursive
    grid split are written in the crate (they are short, and owning them keeps the golden
    tests meaningful). Each crate's use is wrapped so its panics cannot reach the caller
    (inputs are checked first: no NaN, no empty scope, sizes within the limits).

## Consequences

- A library is analysed once; afterwards maps open from the cache in well under a second for
  ten thousand items, and only new or changed files are analysed.
- Five new dependencies.
- The map is only as good as the features. Version 1 favours one-shots (drums, short hits);
  tonal material and loops will need more (chroma, tempo-aware windows) in a later version.
- t-SNE keeps neighbourhoods but not global distances; the spec says so, and "similar" uses
  feature distances, never map distances.

## Alternatives considered

- **UMAP instead of t-SNE:** keeps more of the global structure and is faster on large
  scopes, but the Rust implementations are young (`umap-rs`, `annembed`) or need a GPU stack
  (`fast-umap`). Revisit when one matures; the layout step is behind one function.
- **A learned embedding** (a neural network over spectrograms, or a pretrained audio model run
  with `tract`): better kinds and similarity, but a model to ship and a training set whose
  licence must allow it. Kept for a later version; openly licensed data (CC-BY or CC0) only.
- **Essentia** for features: complete and tested, but AGPL and C++, and its pretrained models are
  under non-commercial terms. **bliss-audio**: Rust and GPL-3.0-only, but made for whole songs and
  would fix the program's licence to GPL-3.0-only.
- **Self-organising maps** for the grid: a grid by construction, but slower and with layouts
  that depend on training order.
