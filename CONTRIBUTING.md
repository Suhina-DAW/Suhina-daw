# Contributing

- **Language:** the product is Rust only. Scripts in `scripts/` are Python, and the FM synth's
  reference renderer is C++; neither is built into the program.
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
- **Issues:** small and scoped, with acceptance criteria (the issue forms ask for them: Bug, Plug-in problem, Feature request, New effect or
  instrument, Preset contribution, Task);
  architecture belongs in an ADR under `docs/decisions/`, not in an issue thread. Every issue has
  a type label (`bug`, `enhancement`, `documentation`, `question`) and one or more area labels
  (`engine`, `ui`, `formats`, `browser`, `audio-io`, `plug-ins`, `devices`, `presets`, `tests`),
  plus `windows`, `macos` or `linux` when it concerns one platform. The forms set the type, and
  `.github/workflows/issue-labels.yml` adds area and platform labels from the form's answers.

## Working on an issue

A task's state lives in its issue and on the [Suhina project board](https://github.com/orgs/suhina-daw/projects/1),
never in chat or in one person's window. Before you write code, three checks:

1. **No open pull request closes it.** The issue's *Linked pull requests* box, or
   <https://github.com/suhina-daw/suhina-daw/pulls?q=is%3Apr+is%3Aopen>, must not show one. A pull
   request that says `Closes #N` has taken it.
2. **Its board item is in `Backlog` or `Ready`.** `In progress` means someone is already working on
   it, and `Do not start yet` means the order is deliberate: the issue and its comments say what the
   work waits for, so ask there instead of starting.
3. **Claim it in a comment** on the issue, and set the item's Status to `In progress` if you can:

   > I'd like to take this. Plan: … When: …

   A claim bot ([`docs/claim-bot.md`](docs/claim-bot.md)) does the same two things for a
   comment that starts with `/claim`, and refuses the claim when an open pull request closes the
   issue; `/unclaim` puts the card back, and a maintainer holds work with `/hold <why>` and opens it
   again with `/ready`. Either way the comment is the claim: a bot that is down never means the
   issue is free.

The first comment wins. If someone was faster, take another issue; if their pull request has
stalled, offer in that pull request to take the work over rather than opening a second one. If you
stop, say so in the issue and set the item back to `Ready`: an issue that looks taken but is not
costs more than an honestly free one.

`good first issue` marks work that needs no knowledge of the engine, `help wanted` work the
maintainers would like a hand with. An `epic` is a container for other issues: claim one of its
issues, not the epic.

**An agent claims and reports like a person does.** An issue is the only context an agent gets
apart from `AGENTS.md` and the documents those point at, so write it to stand on its own — goal,
acceptance criteria, where the code goes — and keep the plan in the issue and the result in the
pull request. The rules in [AI-assisted contributions](#ai-assisted-contributions) apply to what
the agent wrote.

## Trying a pull request

A pull request can be tried without building it: GitHub Actions builds packages of it for
Windows, macOS and Linux ([`docs/releasing.md` › Test builds](docs/releasing.md#test-builds)) and
links the downloads from the pull request in a comment, updated with every new build. A maintainer's own pull requests are
built on every push. A contributor's pull request is built once a maintainer has read the diff and
added the `test-build` label; a later push takes the label off, and the new commits are built when
the label is added again.

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
- Public documentation: user manuals, the officially published Live Object Model documentation,
  published specs (DAWproject, MIDI, CLAP, VST3), papers and textbooks.
- Open-source code under a licence compatible with GPL-3.0-or-later, ported with its notices
  kept. Code without a licence may not be copied, ported or translated; only its published
  description (a paper, a README) may be learnt from.

**Not allowed:**

- Disassembling, decompiling or debugging the binaries of Ableton Live or any other proprietary
  DAW or plug-in, by hand or with tools (Ghidra, IDA, Hopper, REA, ida-pro-mcp, …). If you have
  done this for a product, do not contribute code for that product's features.
  Running Suhina's own processes (the app, the CLI, the plug-in host) under a debugger is
  allowed, also to see that a crash happened in a third-party module; stepping into,
  disassembling or reading that module's code, or drawing conclusions about its internals from
  it, is not. A fix then follows public documentation (for example what Windows requires of a
  window thread), not the module.
- Reading or using leaked source code, or decompiled scripts of a proprietary program (for
  example the control-surface scripts that circulate decompiled).
- Notes, patches, write-ups or tools that come from someone else's debugging, disassembly or
  decompilation of a proprietary program, for example the notes and binary patches of projects
  that make a commercial DAW run on another operating system. An unmodified, licensed copy of
  the program may still be run on such a platform to observe it; the project's findings about
  the program's internals, and its patched copies, may not be read or used.
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

## Effects, instruments and presets

- **A new built-in effect or instrument** follows [ADR 0004](docs/decisions/0004-built-in-devices.md):
  propose it with the *New effect or instrument* issue form, then open the pull request with the
  [device template](https://github.com/suhina-daw/suhina-daw/compare/main...main?template=device.md)
  (choose your branch on that page).
- **Presets** for them go in `presets/<device id>/` under CC0 1.0
  ([ADR 0014](docs/decisions/0014-stock-presets.md)); open the pull request with the
  [preset template](https://github.com/suhina-daw/suhina-daw/compare/main...main?template=preset.md).
  Presets need no clean-room review; their names don't name commercial products, and banks or
  files from commercial products are not accepted.

## AI-assisted contributions

AI coding agents are welcome, under these rules:

- **Credit the model.** Keep the `Co-Authored-By:` trailer your tool adds (for example
  `Co-Authored-By: Claude <noreply@anthropic.com>`), so the history shows how each change was made.
- **Steps anyone can follow.** A change in what Suhina does, in the app, its sound, the files it
  reads and writes, the CLI or MCP, comes with a *How to check it* section in the pull request:
  numbered steps from opening Suhina with nothing loaded, every control named as it reads on
  screen and where it is, every command ready to copy, and what should happen after each step,
  written so that someone who has never used a music program can check it from the test build.
  [`docs/how-to-check.md`](docs/how-to-check.md) shows how to write them.
- **You are responsible for what you submit.** Before you open a pull request, run the tests and
  try every change by hand to see that it works as intended; you answer review questions, not
  the agent.
- **The clean-room rules apply to agents too.** Never give an agent decompiled, disassembled or
  leaked material, and never point it at proprietary binaries. An agent that has seen such
  material must not write Suhina code.
