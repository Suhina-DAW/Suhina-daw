## What

<!-- What changes and why, in a few lines. Link the issue it closes: "Closes #123". -->

## Where the knowledge comes from

<!-- For behaviour or formats of another product: the spec under docs/spec/ this follows
     (written before the code), and how it was found (using the program, files you authored,
     public documentation). For original work: "own design". -->

## Checklist

- [ ] I have not used disassembled, decompiled or leaked material, notes or patches based on
      such material, or content (samples, presets, artwork, icons, interface text, visual design)
      of a proprietary product, and I have not given any of it to an agent.
- [ ] Every commit is signed off (`git commit -s`, [DCO](../CONTRIBUTING.md#developer-certificate-of-origin)).
- [ ] Tests cover the change (round-trip and malformed input for formats, offline renders for
      DSP and engine behaviour).
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and
      `cargo test --workspace` pass.
- [ ] Documents that describe what changed are updated (`python scripts/docs.py impact`).
- [ ] If an AI agent wrote part of it: I have read and understood every line.
