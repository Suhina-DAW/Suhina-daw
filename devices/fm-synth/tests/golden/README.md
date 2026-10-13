# Golden renders of the FM synth

`renders_like_the_original_engine` (in `src/tests.rs`) plays every scenario in
[`scenarios.txt`](scenarios.txt) through the port and compares it, sample for sample, with the
render of the same scenario by the original MSFA engine as Dexed ships it. The port must match
to within 1e-6.

- `scenarios.txt`: the voices, performance settings and notes. The voices were written for
  these tests; none comes from a product's library. `init` is the standard initialised voice
  (full envelopes, operator 1 alone sounding), a firmware default rather than a library voice.
- `<name>.f32`: the reference renders, little-endian 32-bit floats, one channel, every n-th
  frame as the scenario says.

## Making the renders again

The reference renderer is [`../reference/harness.cpp`](../reference/harness.cpp): it builds
the original engine files with stubs for what Dexed's plug-in adds (scale tuning, MTS-ESP,
logging) and the same voice handling as the port. The renders here were made from Dexed commit
`eebb748b70c5a1338afa3945e8b1ea783c9c02fe` (2026-10-10), with g++ 14 (MinGW) on Windows x86-64.

```sh
git clone https://github.com/asb2m10/dexed
mkdir -p ref/msfa
cd dexed/Source/msfa
cp dx7note.cc dx7note.h env.cc env.h exp2.cc exp2.h fm_core.cc fm_core.h fm_op_kernel.cc \
   fm_op_kernel.h freqlut.cc freqlut.h lfo.cc lfo.h pitchenv.cc pitchenv.h porta.cpp porta.h \
   sin.cc sin.h synth.h aligned_buf.h controllers.h module.h ../../../ref/msfa/
cd ../../../ref
cp -r <suhina>/devices/fm-synth/tests/reference/stubs/* .
cp <suhina>/devices/fm-synth/tests/reference/harness.cpp .
g++ -O1 -fwrapv -std=c++17 -I. -Imsfa harness.cpp msfa/dx7note.cc msfa/env.cc msfa/exp2.cc \
   msfa/fm_core.cc msfa/fm_op_kernel.cc msfa/freqlut.cc msfa/lfo.cc msfa/pitchenv.cc \
   msfa/porta.cpp msfa/sin.cc -o harness
./harness <suhina>/devices/fm-synth/tests/golden/scenarios.txt <suhina>/devices/fm-synth/tests/golden
```

`-fwrapv` makes signed overflow wrap, as the engine assumes. To try scenarios of your own
without changing these files, render them into another folder and run

```sh
SUHINA_FM_SCENARIOS=<folder>/scenarios.txt cargo test -p suhina-device-fm-synth -- --ignored
```

## Where the port differs on purpose

These are outside what the scenarios exercise, so the renders still match:

- A parameter change reaches a sounding note without restarting its envelopes; the original
  jumped every held note's envelopes to stage 3.
- Arithmetic that overflowed in C at sample rates below about 16 kHz (the frequency table, the
  rising envelope step) is done in 64 bits, and shifts that would go out of range are limited.
- Unlike the Dexed plug-in, output keeps full resolution instead of being cut to 16 bits per
  voice, and there is no output filter.
- Standard tuning only; one bend range for up and down.
