# Steinberg ASIO SDK (vendored part)

The host side of the Steinberg ASIO SDK 2.3.4 (2025-10-15), from
<https://www.steinberg.net/asiosdk>, used under the **GNU General Public License version 3**,
one of the two licences Steinberg offers it under (see `LICENSE.txt`). Suhina builds it into
cpal's ASIO host when built with the `asio` feature (README › Building); the build finds it
through `CPAL_ASIO_DIR`, set in `.cargo/config.toml`.

Only what a host needs is kept: `common/` (the interface, without the driver-side files) and
`host/` (the driver list and sample conversion), with Steinberg's `LICENSE.txt`, `README.md` and
`changes.txt` as they come. Left out:

- `common/combase.*`, `common/dllentry.cpp`, `common/wxdebug.h`: Microsoft's COM base classes
  for writing a driver, under Microsoft's own terms, not Steinberg's; a host does not use them.
- `common/asiodrvr.*`, `driver/`: writing a driver.
- `host/sample/`, `asio/`, the PDFs and the logo artwork: examples, documents and trademarks.
  Using the ASIO name and logo is governed by Steinberg's usage guidelines, not by the GPL.

The files are unchanged. To update: download the SDK, copy the same files over, and run
`cargo build -p suhina-output --features asio`.
