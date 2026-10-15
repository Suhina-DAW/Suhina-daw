---
description: How to author the FL Studio projects (.flp) that confirm the format spec and test crates/flp, series by series
read_when: making .flp test files, flp corpus, what to save in FL Studio, corpus/flp, flp series
type: howto
status: draft
last_verified: 2026-10-15
covers:
  - crates/flp/tests/corpus.rs
---

# Making the `.flp` test corpus

How to author the FL Studio projects that turn the **(hypothesis)** statements in
[`flp-format.md`](flp-format.md) into confirmed ones, and that `crates/flp` is tested against.
Read [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room first: every file is made by you, in
your own copy of FL Studio, from scratch.

## Rules

- **Change one thing per file.** Each file is the previous one plus exactly one edit, saved under
  a new name, so the events that differ between neighbours are the ones that edit wrote.
- **Only your own content.** No samples, presets or demo projects that come with FL Studio or a
  plug-in. The Sampler channel the Empty template brings stays; it is part of the template.
- **Don't click around.** FL Studio saves window positions and selections too, and every extra
  click adds events to the difference.
- **Never commit the files.** `corpus/` is gitignored. The files carry your licensee name
  (event 200); nothing that prints events may print it.
- **Record what you did** in `corpus/flp/README.md`: the FL Studio version (Help › About), the
  operating system, and anything you did differently from the table.

## Setup

1. Create the folder `corpus/flp/` in your Suhina checkout.
2. Start from File › New from template › Minimal › Empty.
3. Save every step with File › Save as… into `corpus/flp/`, under the name in the table.

## Series 1: basics

| # | File name | Edit from the previous file |
|---|---|---|
| 00 | `00-empty.flp` | No edit: save the Empty template as it opens. Its tempo is FL Studio's default, 140. |
| 00b | `00b-empty-resaved.flp` | **No edit.** Open `00-empty.flp` again and save it under the new name: what changes is noise. |
| 01 | `01-tempo-128.flp` | From `00-empty`: set the tempo to 128. |
| 02 | `02-channel.flp` | From `00-empty`: add a second Sampler channel (the **+** under the channel rack › Sampler). |
| 03 | `03-pattern-note.flp` | In Pattern 1, open the piano roll of the first channel and draw one note at the start of the bar. |
| 04 | `04-playlist-pattern.flp` | Place Pattern 1 at the start of the first playlist track. |
| 05 | `05-mixer-insert.flp` | Route the first channel to mixer insert 1 and rename the insert `Test`. |

## Running the tests

```sh
cargo test -p suhina-flp --features corpus
```

A missing file fails the test with its name and a pointer to this guide. Every other `.flp` in
`corpus/flp/` is read as well, so files from your own experiments are checked for a clean walk too.
