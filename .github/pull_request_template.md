## What and why

<!-- A few lines. Link the issue it closes: "Closes #123". -->

## Where the knowledge comes from

<!-- Own design, or for another product's behaviour or file format: the spec in docs/spec/ this
     follows (written before the code) and how it was found (using the program, files you made
     yourself, public documentation). -->

## How I tested it

<!-- What you tried by hand, on which OS, and what you saw or heard. For sound: the audio device
     and buffer size. -->

## Checklist

**Clean-room**
- [ ] Nothing here comes from disassembled, decompiled or leaked material, or from the content
      of a proprietary product (samples, presets, artwork, icons, interface text, visual design),
      and none of it was given to an agent. Screenshots used only to see where things are are
      fine ([ADR 0009](https://github.com/juhamikael/Loopwerk/blob/main/docs/decisions/0009-ui-toolkit-and-look.md)).
- [ ] Every commit ends with a `Signed-off-by:` line (`git commit -s`): you confirm you may contribute
      the code under the project's licence ([DCO](https://github.com/juhamikael/Loopwerk/blob/main/CONTRIBUTING.md#developer-certificate-of-origin)).

**Works and is tested**
- [ ] I tried every change by hand and it works as intended, also the parts an AI agent wrote.
- [ ] Tests cover it: round-trip and malformed input for file formats, offline renders for sound.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
      and `python scripts/rename_suhina.py --check` pass.

**Fits the architecture** ([AGENTS.md › Golden rules](https://github.com/juhamikael/Loopwerk/blob/main/AGENTS.md#1-golden-rules))
- [ ] New things a user can do to a project are commands, so an agent can do them over MCP and
      core features work from the CLI (e.g. moving a clip, setting a track's volume, grouping
      tracks). Purely visual changes need none.
- [ ] Nothing on the audio thread allocates, locks, blocks, logs or does I/O, and no new code can
      panic.

**Documented**
- [ ] The documents that describe what changed are updated (`python scripts/docs.py impact`).
