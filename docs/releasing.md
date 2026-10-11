# Releasing

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml).

## Making a release

1. Set the version in the root `Cargo.toml` (`[workspace.package] version`), run
   `cargo check` so `Cargo.lock` follows, and merge that through a pull request.
2. Tag the merge commit on `main` with the same version and push the tag:

   ```sh
   git tag v0.0.2
   git push origin v0.0.2
   ```

3. The workflow checks that the tag matches the version, builds the packages, runs each binary's
   `--version` and checks its library dependencies, and creates a **draft** pre-release with the
   archives and their SHA-256 sums attached.
4. Read the draft on GitHub, write the notes and publish it.

Running the workflow by hand (Actions › Release › Run workflow) builds the same packages as
workflow artifacts without creating a release, to test the pipeline.

## Packages

| Package | Built on | Contents |
|---|---|---|
| `loopwerk-<version>-linux-x86_64.tar.gz` | Ubuntu 22.04 (glibc 2.35) | `loopwerk-cli`; needs `libasound2` |
| `loopwerk-<version>-windows-x86_64.zip` | Windows, MSVC, static C runtime | `loopwerk-cli.exe`; no Visual C++ redistributable needed |
| `loopwerk-<version>-macos-universal.tar.gz` | macOS, arm64 + x86_64 joined with `lipo` | `loopwerk-cli`; macOS 11 or later |

Each archive also holds `LICENSE`, `NOTICE`, `README.md` and `ATTRIBUTION.md`.

## Not yet

The binaries are not signed: macOS Gatekeeper and Windows SmartScreen warn about them. Installers
(AppImage, deb, rpm, Flatpak, MSI), code signing and notarisation, and Linux aarch64 and Windows
arm64 builds come with the UI (ROADMAP, M8).
