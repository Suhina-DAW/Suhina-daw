---
description: How the sample map analyses samples (features), lays them out (PCA and t-SNE, grid assignment), finds similar ones (a linear scan), caches the results, and which crates it uses
read_when: sample map, soundmap crate, audio features, MFCC, spectral centroid, t-SNE, bhtsne, PCA, UMAP, grid layout, RasterFairy, nearest neighbours, feature cache
type: decision
status: current
last_verified: 2026-10-13
covers:
  - crates/soundmap/**
  - crates/session/src/soundmap.rs
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

1. **A crate of its own, `suhina-soundmap` (layer 2, beside `library`)**, depends on `media`
   (decoding) only. The session feeds it the library's files and watches the library's revision
   (`Library::files`, `Library::revision`), so it lays out again when files change. It holds
   the analysis queue and threads, the feature cache, the layouts and the neighbour search. The
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

   Times are in log seconds, so short sounds are told apart; frequencies in log2 Hz. Each
   feature is tested on signals whose answer is known (a sine's pitch and centroid, noise's
   flatness and zero crossings, a short hit's length, the same result for mono and stereo and
   across sample rates). The feature list has a version; a cache entry of another version is
   analysed again.
3. **Distances** are Euclidean over features standardised across the scope (each one to mean 0,
   deviation 1), with the four groups (MFCC, spectral shape, envelope, pitch) weighted equally
   regardless of how many numbers they hold.
4. **Layout:** PCA down to 20 dimensions (the covariance matrix's eigenvectors by Jacobi
   rotations, each with a fixed sign), then Barnes-Hut t-SNE (θ 0.5, perplexity 30, or a third
   of the item count when that is smaller; 1 000 iterations), started from the first two
   principal components with a little jitter seeded from the scope's paths. Scopes under 16
   items use the first two components alone. The result is scaled to fit 0–1, keeping its
   aspect. t-SNE sums in parallel, so two runs can differ in the last digits; therefore a
   layout is kept, in memory and in `layout-<hash>.bin` beside the caches (the 32 newest), by a
   hash of the scope's items with their sizes and times: a scope opens with the map it had, and
   changes only when its items do. Measured on one machine: 1 000 items in 0.5 s, 10 000 in
   4.5 s, 50 000 in 18 s.
5. **Grid:** recursive splitting (the RasterFairy method): the cells are cut in two along the
   longer side and the points, sorted along the same axis, are shared between the halves in
   proportion to their cells, until a part holds at most 256 items. Each part's points are
   stretched over its cells and assigned exactly by the Hungarian method (least total squared
   distance from point to cell centre). 50 000 items take about a second.
6. **Neighbours:** "similar items" is a linear scan over the standardised features (under 2 ms
   for 50 000 items); no tree is needed at these sizes. Finding the point under the pointer is
   the view's job.
7. **Kinds** come from words in names (the spec's table). An item without one takes the kind
   most of its 7 nearest neighbours have, when 4 or more agree. No trained model in this
   version.
8. **Cache** in `<settings>/library/features-<hash>.bin` beside the library's index, one file
   per folder: a version header with the feature version, then per file its path below the
   folder, size, modification time and either its length in seconds and its features (32-bit
   floats) or the reason it could not be analysed. The layout shows only files no longer than
   the length limit, so changing the limit needs no new analysis. An entry whose size or time no longer
   matches the library's index is analysed again. Written to a file beside it and moved over
   it, as the index is. A file that does not parse is ignored and the folder analysed again.
9. **Threads:** analysis runs on a pool of half the cores (at least one), started by
   `Library::start`'s caller as indexing is, so tests and the CLI never analyse a user's
   folders unasked. A file larger than 256 MiB is not decoded. Caches are written after every
   256 analysed files of a folder and when the queue empties.
10. **Crates**, both MIT: `realfft` for the transforms and `bhtsne` for t-SNE. MFCC, the
    spectral features, YIN, PCA, the grid split and the Hungarian method are written in the
    crate: each is short, and owning them avoids pulling in a matrix library. `bhtsne` checks
    its inputs with assertions, so its inputs are checked first (perplexity within the item
    count, finite values) and the call is wrapped in `catch_unwind`; a failure falls back to the
    first two principal components.

## Consequences

- A library is analysed once; afterwards maps open from the cache in well under a second for
  ten thousand items, and only new or changed files are analysed.
- Two new dependencies.
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
