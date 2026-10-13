---
description: How versions are numbered, how release builds and the nightly for Linux, Windows and macOS are made
read_when: release, version, release candidate, tag, binaries, release workflow, nightly, development build, download, Discord announcement, release notes, changelog
type: howto
status: current
last_verified: 2026-10-13
covers:
  - .github/workflows/release.yml
  - scripts/release_notes.py
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
4. Read the draft on GitHub, edit the notes (see [Release notes](#release-notes)) and publish it.
5. For the next candidate, or the final release, repeat from step 1 with `0.1.0-rc.2` or `0.1.0`.

Running the workflow by hand (Actions › Release › Run workflow) builds the same packages as
workflow artifacts without creating a release, to test the pipeline.

## Nightly

The nightly is how people try the current state between releases: a build of `main`, not a
release. Every night (02:17 UTC) the workflow builds `main` and publishes it as the pre-release
**`nightly`**, replacing the previous one: its release and tag are deleted and made again on the
new commit, published at once without a draft. A night with no new commit on `main` builds
nothing. Running the workflow by hand from `main` with **nightly** checked publishes one now.

The nightly's version is the workspace version without a pre-release suffix, the date and the
commit: `0.0.1-nightly.20261013+a15ad35`. The workflow writes it into `Cargo.toml` and
`Cargo.lock` before building (nothing is committed), so `suhina-cli --version` and bug reports
say which build it is. Its packages keep one name, `suhina-nightly-<platform>`, so a link to
the latest one never changes:

```text
https://github.com/suhina-daw/suhina-daw/releases/download/nightly/suhina-nightly-linux-x86_64.tar.gz
```

Each new nightly is posted to Discord: the version, the download link and the changes since the
previous nightly (at most 15, as described under [Release notes](#release-notes)). The post goes to the channel whose webhook
URL is the repository secret `DISCORD_WEBHOOK` (Discord: channel settings › Integrations ›
Webhooks; GitHub: Settings › Secrets and variables › Actions). Without the secret nothing is
posted, and a failed post leaves the nightly published. Releases from version tags are not
posted; they are announced by hand when the draft is published.

The nightly never becomes the repository's "latest" release, and nothing is promoted from it: a
release is made from a version tag as above.

## Release notes

[`scripts/release_notes.py`](../scripts/release_notes.py) writes the notes from git alone (no
network, no AI service). A change is one commit on `main`'s first-parent line, so one squash-merged
pull request, listed by its title with a link to the pull request. Changes that touch only
documentation, CI, scripts and tests are counted, not listed. Pull request titles are the release
notes, so they say what changed for a user: "Clip fades: shapes and an adjustable curve".

- **Nightly:** the changes since the previous nightly, on its release page and in the Discord post.
- **Draft of a tagged release:** first *New and changed*, the bullets of ROADMAP.md › What works now
  that are new or reworded since the previous release, by milestone, then *All changes*. A
  release candidate counts from the tag before it, a release from the previous release, the first
  from the beginning. Edit the draft before publishing: the ROADMAP bullets describe what works,
  not what changed, and a reworded bullet may be a fix of its text.

Its tests run in CI: `python3 -m unittest discover -s scripts -p 'test_*.py'`.

## Packages

| Package | Built on | Contents |
|---|---|---|
| `suhina-<version>-linux-x86_64.tar.gz` | Ubuntu 22.04 (glibc 2.35) | `suhina`, `suhina-cli`, `suhina-plugin-host`; needs `libasound2` |
| `suhina-<version>-windows-x86_64.zip` | Windows, MSVC, static C runtime | `suhina.exe`, `suhina-cli.exe`, `suhina-plugin-host.exe`; no Visual C++ redistributable needed |
| `suhina-<version>-macos-universal.tar.gz` | macOS, arm64 + x86_64 joined with `lipo` | `suhina`, `suhina-cli`, `suhina-plugin-host`; macOS 11 or later |

The nightly's packages are named `suhina-nightly-<platform>` instead. `suhina` is the desktop app,
`suhina-cli` the command-line tool. `suhina-plugin-host` is the process plug-ins run in
([ADR 0006](decisions/0006-plug-in-hosting.md)); both look for it in their own folder, so the
three stay side by side. The smoke test runs the VST3 test plug-in built into the host through the
packaged CLI; the desktop app needs a window, so only its libraries are checked. Each archive also holds
`LICENSE`, `NOTICE`, `README.md`, `ATTRIBUTION.md` and the licences of the ported code,
`LICENSE-APACHE` (the FM synth) and `LICENSE-signalsmith.txt` (the time stretcher).

## Not yet

The binaries are not signed: macOS Gatekeeper and Windows SmartScreen warn about them. The
desktop app is a bare executable, not an installer or a macOS app bundle. Installers (AppImage,
deb, rpm, Flatpak, MSI), a macOS `.app`, code signing and notarisation, and Linux aarch64 and
Windows arm64 builds come with M8 (ROADMAP).
