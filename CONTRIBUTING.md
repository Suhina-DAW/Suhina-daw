# Contributing

- **Language:** Rust only.
- **Licence:** Suhina is GPL-3.0-or-later (`LICENSE`, `NOTICE`), and contributions are
  licensed the same way. Dependencies and bundled libraries must be GPLv3-compatible (MIT,
  Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL, GPL-3.0); GPL-2.0-only and proprietary or
  non-commercial licences are not.
- **Never crash:** non-test code must not panic and must not use `unsafe` (the only exceptions
  are the plug-in hosting crates `ipc` and `plugin-host`, ADR 0006). Errors go through `Result`
  and `?`; input-derived indices and sizes are checked. See `AGENTS.md`.
- **Real-time safety:** code that runs on the audio thread never allocates, locks, blocks or does
  I/O. Communicate with it through lock-free queues.
- **Tests:** required for every change. Format code needs round-trip and malformed-input tests;
  DSP and engine code is tested by rendering offline and comparing against golden files.
- **Documentation:** a change that alters how something works updates the documents that
  describe it in the same pull request (architecture, ADRs, specs, ROADMAP); `AGENTS.md` ›
  *Before you finish a task* lists which document covers what.
- **Style:** `cargo fmt`, and `cargo clippy --all-targets -- -D warnings`. Comments explain *why*.
- **Names:** packages are `suhina-<name>` (`suhina-device-<name>` for devices), used in code as
  `suhina_<name>`; directories have no prefix. `AGENTS.md` › *Crate names* says how a new crate
  is added; CI rejects the project's old name (`python scripts/rename_suhina.py --check`).
- **Commits:** small, focused, with a clear subject line, and signed off (see
  [Developer Certificate of Origin](#developer-certificate-of-origin)).
- **Issues:** small and scoped, with acceptance criteria (the issue template asks for them);
  architecture belongs in an ADR under `docs/decisions/`, not in an issue thread.

## Clean-room

Suhina reimplements the *behaviour* of commercial DAWs, Ableton Live in particular, without
using any of their code or content. This rule outranks feature work: one contaminated change can
force a rewrite of everything built on it, so it is not something we clean up later.

**Allowed sources:**

- Using the software normally and observing what it does: playback, timing, quantisation, how
  clips launch, how parameters respond. Recording and measuring its audio and MIDI output.
- Project, clip and preset files **you authored yourself** (`.als`, `.alc`, `.adg`, `.adv`).
  These are gzip-compressed XML; study them by saving variants that differ in one setting and
  diffing them.
- Public documentation: user manuals, the Live Object Model and Remote Script documentation,
  published specs (DAWproject, MIDI, CLAP, VST3), papers and textbooks.

**Not allowed:**

- Disassembling, decompiling or debugging the binaries of Ableton Live or any other proprietary
  DAW or plug-in, by hand or with tools (Ghidra, IDA, Hopper, REA, ida-pro-mcp, …). If you have
  done this for a product, do not contribute code for that product's features.
- Reading or using leaked source code.
- Notes, patches, write-ups or tools that come from someone else's debugging, disassembly or
  decompilation of a proprietary program, for example the notes and binary patches of projects
  that make a commercial DAW run on another operating system. Such a project may still be used
  as a test platform to run the program and observe it; its findings about the program's
  internals may not be read or used.
- Copying samples, presets, Packs, Core Library content, impulse responses, wavetables or demo
  projects from proprietary products, or committing them as test data.
- Copying the visual design of a proprietary product: icons, artwork, interface text, colours,
  typefaces or the look of its controls. Familiar workflow and the structure of the window (which
  region is where, what a view shows, how it reacts, its shortcuts and menu structure) are
  allowed; [`docs/design/brief.md`](docs/design/brief.md) lists what stays familiar and what
  must be Suhina's own. Screenshots of a proprietary product may be used as a reference for
  structure and behaviour, by people and by design tools, but the look drawn from them must follow
  the brief, not the screenshot; they are never traced or committed, and nothing in them (icons,
  artwork, text) is reused ([ADR 0009](docs/decisions/0009-ui-toolkit-and-look.md)).
- Using the Ableton or Live names, logos or trade dress for Suhina itself.

**How we work:** findings go into prose specs under `docs/spec/` (your own words, no code from
the product). Implementation is written from those specs. AI agents follow the same rules: an
agent that writes Suhina code must never be given decompiled or leaked material.

If you are unsure whether a source is allowed, ask in an issue before using it.

## Developer Certificate of Origin

Every commit in a pull request from outside the project ends with a sign-off line:

```text
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds it. By signing off you certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the
change or have the right to submit it under the project's licence. For Suhina this includes
the clean-room rules above: the change does not come from disassembled, decompiled or leaked
material, from notes based on such material, or from content of a proprietary product, and no
agent that wrote it was given such material. CI checks the sign-off on pull requests from forks.

## Reporting a rights problem

If you believe Suhina contains code, content or design that infringes your rights, or that a
contribution broke the clean-room rules, see [`IP.md`](IP.md). Such reports are handled before
any other work.

## AI-assisted contributions

AI coding agents are welcome, under these rules:

- **Credit the model.** Keep the `Co-Authored-By:` trailer your tool adds (for example
  `Co-Authored-By: Claude <noreply@anthropic.com>`), so the history shows how each change was made.
- **You are responsible for what you submit.** Read and understand every line before you open a
  pull request; you answer review questions, not the agent.
- **The clean-room rules apply to agents too.** Never give an agent decompiled, disassembled or
  leaked material, and never point it at proprietary binaries. An agent that has seen such
  material must not write Suhina code.
