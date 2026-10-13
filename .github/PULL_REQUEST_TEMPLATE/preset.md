## Presets

<!-- Which device, and what each preset is for, in a line each. Link the issue if there is one. -->

## How I tested it

<!-- The preset loader is not built yet (ADR 0014): set each preset's values by hand with
     device.setParam (or in the device view) and listen to it. Once the loader exists, load the
     file (device.add with the preset, or the browser) instead. -->

## Checklist

- [ ] Every preset is a `.json` file in `presets/<device id>/` in the format of
      [ADR 0014](https://github.com/juhamikael/Loopwerk/blob/main/docs/decisions/0014-stock-presets.md),
      with every parameter given and inside its range.
- [ ] I release these presets under CC0 1.0 ([`presets/README.md`](https://github.com/juhamikael/Loopwerk/blob/main/presets/README.md)),
      and every commit ends with a `Signed-off-by:` line (`git commit -s`).
- [ ] Names, tags and descriptions are my own words and don't name commercial products or their
      presets; no banks or files from commercial products are included.
- [ ] I listened to every preset (its values set by hand until the loader exists);
      `cargo test --workspace` passes.
