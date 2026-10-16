---
description: How the browser shows installed plug-ins: grouped by vendor or by type, or as one flat list with sortable columns; and how each format's own categories map to Suhina's groups
read_when: plug-in views, browser plug-ins, group by type, vendor, flat list, sorting, columns, VST3 sub-categories, CLAP features, favourites, tags
type: spec
status: current
last_verified: 2026-10-14
covers:
  - crates/ui/src/browser.rs
  - crates/ui/src/browser/browser_view.rs
  - crates/session/src/rack.rs
  - crates/session/src/library.rs
  - crates/library/src/store.rs
---

# Plug-ins in the browser

Every installed plug-in the scan found (`docs/spec` › the plug-in catalog, ADR 0006 › 9) is
listed in the browser and can be dragged onto a track from any view of it. The plug-ins appear
in the tree under the **Plug-ins** heading, in the **Plug-ins** category
(the **Show Plug-ins** filter), and in searches, pins and tag filters like any other item
([browser](browser.md) › Commands, `library.search`).

## The three views

When the category is **Plug-ins**, a switch above the results chooses how they are shown, as
the List | Map switch does for the sample map:

| View | What it shows |
|---|---|
| **Grouped** (default) | The tree as ever: a **Plug-ins** heading, the vendors under it in natural order (unknown vendors last), each vendor's plug-ins under the vendor, by name. |
| **By type** | Two headings, **Instruments** and **Effects**; under each, Suhina's groups (the tables below); under each group, that group's plug-ins by name. A plug-in appears once, under the first of its groups the tables give; a plug-in with no group sits under **Other**. |
| **All** | One flat list of every plug-in, no headings: columns **Name**, **Vendor**, **Type** (`instrument`/`effect`), **Format** (`vst3`, `clap`), and **Modified** (the date the scan read the file, from the catalog). |

Rows behave the same in every view: select, drag onto a track (one row, or several with the
modifiers), pin, tag, right-click menu. In the flat list every column heading is a button:
clicking it sorts by that column, clicking again reverses; the name column sorts in natural
order, the others by their text (vendor and format), type (instrument before effect) and date.
The choice of view is kept with the library (browser.json, `plugins_view`), so it is there on
the next start; the sorting choice is the window's own and is not kept.

The views replace the tree only where the plug-ins are listed alone (the **Plug-ins**
category). In **All**, **Instruments** and **Effects** categories the plug-ins stay in the
tree, grouped by vendor, as today.

## Suhina's groups, and how each format's categories map to them

A plug-in's type is `instrument` or `effect` (`suhina_session::is_instrument`: any of its
features containing "instrument"). Under the type, the plug-in's own sub-categories name its
group — the first of these that matches, in the order here; none matches, **Other**:

| Suhina's group | VST3 sub-category (`Fx …`/`Instrument …`) | CLAP feature |
|---|---|---|
| Instrument › Synth | `Synth` | `synthesizer` |
| Instrument › Sampler | `Sampler` | `sampler` |
| Instrument › Drums | `Drum` | `drum-machine` |
| Effect › Reverb | `Reverb` | `reverb` |
| Effect › Delay | `Delay` | `delay` |
| Effect › EQ | `EQ` | `eq` |
| Effect › Filter | `Filter` | `filter` |
| Effect › Dynamics | `Dynamics`, `Compressor`, `Limiter`, `Gate` | `compressor`, `limiter`, `gate` |
| Effect › Distortion | `Distortion` | `distortion`, `saturation` |
| Effect › Modulation | `Modulator`, `Chorus`, `Flanger`, `Phaser`, `Tremolo`, `Vibrato` | `modulator` |
| Effect › Analyzer | `Analyzer` | `analyzer` |
| Effect › Utility | `Utility`, `Spatial` | `utility` |
| (else) Other | — | — |

VST3 sub-categories are the scan's `features` (the words between the `|` of
`moduleinfo.json`'s *Sub Categories*, or of `PClassInfo2`); CLAP's are its features as the scan
read them. A VST3 word matches as a whole word, case-insensitively (`Fx|Reverb` gives the
Reverb group; `Fx|Dynamics|Compressor` gives Dynamics). The names shown are Suhina's own words;
a format's category is used only to pick the group.

## Where the choice is kept

`library.setPluginsView { view: "grouped" | "type" | "flat" }` sets it; the reply says the view
chosen. It is saved in `browser.json` as `plugins_view`
([browser](browser.md) › Where it is kept), read with the same rules as the rest of the file: a
missing or unknown value falls back to `grouped` with a warning, never a failure.

## Favourites and tags

Pins and colour tags work on a plug-in's row as on a file: the row's key is
`plugin:<spec>` ([browser](browser.md) › Terms), which the pin list and the tag store keep like
any key; the **Pinned** place and the footer's tag filters list tagged and pinned plug-ins with
the rest. The tags are the user's own named tags with colours
(`library.setTag`, `library.tag`); a plug-in can carry any of them, and filtering by a tag
shows the plug-ins that carry it among the files.
