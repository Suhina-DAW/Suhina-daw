# Golden renders of the engine

Tests in `src/fade_tests.rs` render small projects offline and compare the left channel, sample
for sample (within 1e-6), with the files here. Offline renders are the oracle for real-time
playback: both run the same `Renderer::process` (AGENTS.md › One render path).

- `fade-in.f32`: a 2-beat audio clip at 120 bpm, 8 kHz, playing a constant 0.25, with a
  1-beat fade-in.
- `fade-out.f32`: the same clip with a half-beat fade-out.

Each file is little-endian 32-bit floats, one channel, one per frame. The tests also check every
frame against the envelope written out in the test (linear fades times the 2 ms click guard,
`docs/spec/arrangement-editing.md` › Fades), so a golden file cannot hold a wrong envelope.

## Making the renders again

Only when a change to the envelope is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-engine fade_tests
```
