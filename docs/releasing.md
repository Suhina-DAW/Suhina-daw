---
description: How versions are numbered and release builds for Linux, Windows and macOS are made
read_when: release, version, release candidate, tag, binaries, release workflow
type: howto
status: current
last_verified: 2026-10-12
covers:
  - .github/workflows/release.yml
---

# Releasing

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml).

## Versions

Versions follow [Semantic Versioning](https://semver.org): `MAJOR.MINOR.PATCH`, with release
candidates as `MAJOR.MINOR.PATCH-rc.N`. A release goes out as one or more release candidates first;
the final release is the last candidate unchanged, under the plain version:

```text
0.1.0-rc.1  →  testers find a bug, fixed on main  →  0.1.0-rc.2  →  no new bugs  →  0.1.0
```

The workflow accepts only these two forms. Release candidates, and every release before 1.0, are
published as GitHub pre-releases and never become the repository's "latest" release.

## Making a release

1. Set the version in the root `Cargo.toml` (`[workspace.package] version`), for example
   `0.1.0-rc.1`, run `cargo check` so `Cargo.lock` follows, and merge that through a pull request.
2. Tag the merge commit on `main` with the same version and push the tag:

   ```sh
   git tag v0.1.0-rc.1
   git push origin v0.1.0-rc.1
   ```

3. The workflow checks the version's form and that the tag matches it, builds the packages, runs
   each binary's `--version` and checks its library dependencies, and creates a **draft** release
   with the archives and their SHA-256 sums attached.
4. Read the draft on GitHub, write the notes and publish it.
5. For the next candidate, or the final release, repeat from step 1 with `0.1.0-rc.2` or `0.1.0`.

Running the workflow by hand (Actions › Release › Run workflow) builds the same packages as
workflow artifacts without creating a release, to test the pipeline.

## Packages

| Package | Built on | Contents |
|---|---|---|
| `suhina-<version>-linux-x86_64.tar.gz` | Ubuntu 22.04 (glibc 2.35) | `suhina-cli`, `suhina-plugin-host`; needs `libasound2` |
| `suhina-<version>-windows-x86_64.zip` | Windows, MSVC, static C runtime | `suhina-cli.exe`, `suhina-plugin-host.exe`; no Visual C++ redistributable needed |
| `suhina-<version>-macos-universal.tar.gz` | macOS, arm64 + x86_64 joined with `lipo` | `suhina-cli`, `suhina-plugin-host`; macOS 11 or later |

`suhina-plugin-host` is the process plug-ins run in ([ADR 0006](decisions/0006-plug-in-hosting.md));
`suhina-cli` looks for it in its own folder, so the two stay side by side. The smoke test runs
the VST3 test plug-in built into the host through the packaged CLI. Each archive also holds
`LICENSE`, `NOTICE`, `README.md`, `ATTRIBUTION.md` and the licences of the ported code,
`LICENSE-APACHE` (the FM synth) and `LICENSE-signalsmith.txt` (the time stretcher).

## Not yet

The binaries are not signed: macOS Gatekeeper and Windows SmartScreen warn about them. The
desktop app (`apps/suhina`) is not packaged yet; it, installers (AppImage, deb, rpm, Flatpak, MSI),
code signing and notarisation, and Linux aarch64 and Windows arm64 builds come with M8 (ROADMAP).
