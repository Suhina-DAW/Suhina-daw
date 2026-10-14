---
description: How versions are numbered, how release builds, the nightly and test builds of pull requests are made
read_when: release, version, release candidate, tag, binaries, release workflow, nightly, past nightlies, development build, download, test build, pull request build, try a pull request, Discord announcement, release notes, changelog
type: howto
status: current
last_verified: 2026-10-14
covers:
  - .github/workflows/release.yml
  - .github/workflows/discord.yml
  - .github/workflows/test-build.yml
  - .github/workflows/test-build-links.yml
  - scripts/release_notes.py
  - scripts/discord_post.py
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

The workflow accepts only these two forms. Release candidates are published as GitHub
pre-releases and never become the repository's "latest" release; a release, `0.x` ones included,
becomes the latest.

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
release. Every night (02:17 UTC) the workflow builds `main` and publishes it as the release
**`nightly`**, replacing the previous one: its release and tag are deleted and made again on the
new commit, published at once without a draft. A night with no new commit on `main` builds
nothing. Running the workflow by hand from `main` with **nightly** checked publishes one now.

The nightly's version is the workspace version without a pre-release suffix, the date and the
commit: `0.0.1-nightly.20261013+a15ad35`. The workflow writes it into `Cargo.toml` and
`Cargo.lock` before building (nothing is committed), so `suhina-cli --version`, the desktop
app's Help › About Suhina ([spec](spec/about.md)) and bug reports say which build it is. Its
packages keep one name, `suhina-nightly-<platform>`, so a link to the latest one never changes:

```text
https://github.com/suhina-daw/suhina-daw/releases/download/nightly/suhina-nightly-linux-x86_64.tar.gz
```

Each night is also kept as a pre-release of its own under the tag **`nightly-YYYYMMDD`**
(`nightly-20261013`), with the same packages and notes, published on the same commit and never
the latest. A tester can go back to an earlier night to check when something broke, or return
to a build that opens their project. A second nightly on the same day, one run by hand, replaces
that day's. The nightly job deletes the dated nightlies older than `NIGHTLY_KEEP_DAYS` (30) with
their tags; it touches no other release, and a failure there leaves the new nightly published.

Each new nightly is posted to Discord's releases channel (see [Discord](#discord)): the version,
a link to its dated release, so the link still leads to the build it announced after the next
night, and the changes since the previous nightly (at most 15, as described under
[Release notes](#release-notes)). A failed post leaves the nightly published.

Until Suhina has a release of its own (published, not a release candidate), the nightly is the
repository's **latest** release, so the Releases box on the repository page offers its downloads.
From the first release on, the nightly is published as a pre-release that never becomes the
latest. Nothing is promoted from it: a release is made from a version tag as above.

## Test builds

A test build is a pull request packaged for download, so it can be tried without building it
locally. [`.github/workflows/test-build.yml`](../.github/workflows/test-build.yml) builds the
same three packages as the nightly, for Linux, Windows and macOS (see [Packages](#packages)), and
uploads each as a workflow artifact kept for 7 days.
[`.github/workflows/test-build-links.yml`](../.github/workflows/test-build-links.yml) then links
them from the pull request in one comment that each new build updates, with a platform that failed
linked to its log. Downloading an artifact needs a GitHub sign-in, and an artifact is always a
zip: on Windows it holds the package folder, on Linux and macOS the package's `.tar.gz`, because a
zip drops the executable bit.

| Pull request | Built |
|---|---|
| opened and pushed by a maintainer | on every push; a newer push cancels a build still running |
| anyone else's, from a fork or a branch | when a maintainer adds the `test-build` label, of the commit the label was added on |

Only a pull request that changes what goes into the binaries is built: `apps/`, `crates/`,
`devices/`, `presets/`, `third_party/`, `Cargo.toml`, `Cargo.lock`, `.cargo/` or `test-build.yml`
itself. One that changes only documentation, scripts, other workflows or agent files has no new
binary to try.

The build is of the pull request's head merged with `main`, as the event saw it, so a push after
the label is not what gets built. A push by anyone but a maintainer takes the label off, and the
new commits are built when a maintainer has read them and adds the label again; the comment on such
a build says it is a contributor's. A first-time contributor's run also waits for a maintainer's
*Approve and run*.

The version says which pull request and commit it is: `0.0.1-pr.302+abc1234` in `--version` and
bug reports. Test builds use the release profile without LTO and with 16 codegen units, so they
build faster and run slightly slower than the nightly.

**Several pull requests together:** Actions › Test build › Run workflow with their numbers
(`302 303 304`) merges them onto `main` in that order and builds the result, named
`suhina-prs-302-303-304-<platform>`; each platform's summary links its package. A pull request that does not
merge cleanly fails the run and is named in the error.

The build has a read-only token and no secrets, and for a fork GitHub runs `test-build.yml` as the
fork has it; so `test-build-links.yml`, which runs from `main` and never runs pull request code,
links only runs a maintainer started. The maintainers are listed in both workflows.

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

## Discord

Two channels get posts from GitHub Actions, each through a webhook (Discord: channel settings ›
Integrations › Webhooks) whose URL is a repository secret (GitHub: Settings › Secrets and
variables › Actions). A channel without its secret gets nothing, and a failed post fails nothing
else. No mention in a post pings anyone.

| Channel | Secret | Posts |
|---|---|---|
| releases | `DISCORD_WEBHOOK_RELEASES` | each nightly (`release.yml`); each release when a person publishes it, with its *New and changed* section (`discord.yml`) |
| events | `DISCORD_WEBHOOK_EVENTS` | pull requests opened, merged, closed and reopened; issues opened, closed and reopened (`discord.yml`) |

[`scripts/discord_post.py`](../scripts/discord_post.py) writes the posts of `discord.yml` from
the event. The nightly never reaches `discord.yml`: it is created with the workflow's own token,
which starts no other workflow. `discord.yml` runs on `pull_request_target`, so pull requests from
forks are posted too; it checks out the base branch and never runs the pull request's code. No
repository webhook (Settings › Webhooks) posts to Discord; one would post the nightly twice.

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
