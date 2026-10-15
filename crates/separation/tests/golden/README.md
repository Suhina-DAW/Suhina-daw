# Golden render of stem separation

`src/tests.rs` (`the_test_backend_renders_like_its_golden_file`) separates a fixed input with the
test backend and compares every sample (within 1e-5) with the file here. It covers the whole
pipeline: resampling to the model's rate and back, the windows, the padded tail and the weighted
overlap-add (`docs/spec/stem-separation.md`).

- `band-splitter.f32`: the input is 0.25 s of stereo at 12 kHz, 3000 frames: on the left
  0.3 × 60 Hz + 0.2 × 700 Hz + 0.1 × 3000 Hz sines and a click of 0.5 at frame 1000, on the right
  90 Hz, 900 Hz and 3500 Hz at the same levels. The backend is a `BandSplitter` at 8 kHz, stereo,
  windows of 512 frames every 384, triangle weighting, crossovers at 300 Hz and 1500 Hz: three
  stems. The file holds the three stems one after another, each interleaved left, right, as
  little-endian 32-bit floats (3 × 3000 × 2 values).

The test also checks the render against facts written out in it (the stems add up to the input as
8 kHz can hold it, and the click is in the high stem), so a golden file cannot hold a wrong render.

## Making the render again

Only when a change to the pipeline's output is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-separation golden
```
