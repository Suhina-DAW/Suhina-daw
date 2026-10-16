#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""The documentation index: front matter in every file under docs/, docs/INDEX.md built from it,
and checks on the front matter.

  python scripts/docs.py index            print the index docs/INDEX.md would have
  python scripts/docs.py index --write    rewrite docs/INDEX.md (the workflow on main does this)
  python scripts/docs.py check            front matter, covered paths, and links between documents
  python scripts/docs.py impact [BASE]    documents that describe code changed since BASE
                                          (default origin/main) but were not changed themselves
  python scripts/docs.py stamp            move each document's last_verified up to the day of its
                                          last change on main (the workflow on main does this)

Front matter, at the top of each document:

  ---
  description: one sentence on what the document says
  read_when: the words someone would look for it with
  type: system | decision | spec | howto | plan
  status: current | draft | planned | superseded
  last_verified: yyyy-mm-dd (the day the text was last checked against the code or sources;
                 stamped on main, see below)
  covers:
    - crates/engine/src/launcher.rs   (paths or globs of the code it describes; optional)
  ---

docs/INDEX.md and docs/commands.md are generated on main after every merge
(.github/workflows/docs-generated.yml), so pull requests leave them alone and never conflict on
them. `index` without `--write` only prints, so a local look never ends up in a commit.

last_verified is stamped on main too: a pull request that changes a document has checked it against
the code it changes, so after the merge `stamp` moves the document's date up to the day of that
change. Pull requests leave the line alone, so two of them that touch one document do not conflict
on it. Someone who checks a document without changing anything else in it still sets the date by
hand. Commits made before STAMP_FROM, the docs bot's own commits ("Docs: regenerate ...") and
commits whose message has a line `Last-verified: keep` (a rename across every document, say) do not
count as a check.

Standard library only, so CI needs nothing but Python.
"""

from __future__ import annotations

import fnmatch
import re
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
INDEX = DOCS / "INDEX.md"
# Written by tools, not by hand: no front matter, regenerated on main after every merge.
GENERATED = (
    (INDEX, "This index"),
    (DOCS / "commands.md", "Every command with its kind and parameters, from the command registry"),
)
REQUIRED = ("description", "read_when", "type", "status", "last_verified")
TYPES = ("system", "decision", "spec", "howto", "plan")
STATUSES = ("current", "draft", "planned", "superseded")
STALE_AFTER_DAYS = 90
# `stamp` reads history from this day on: before it, pull requests set last_verified by hand.
STAMP_FROM = "2026-10-17"
# Commits that change documents without checking them: the docs bot's, and those that say so.
STAMP_SKIP = ("^Docs: regenerate", "^Last-verified: keep")
LAST_VERIFIED = re.compile(r"^last_verified:[ \t]*(\S*)[ \t]*$")
# A file covered by this many documents or more is a hub (the session's or the window's main file):
# a change to it alone does not make `impact` warn about each of them, only list them.
HUB_DOCS = 3
# Documents outside docs/ that the index lists by hand; they carry no front matter.
ROOT_DOCS = (
    ("AGENTS.md", "Rules for agents and contributors, the crate map, what to run before finishing"),
    ("CONTRIBUTING.md", "Clean-room rules, licences, AI-assisted contributions"),
    ("ROADMAP.md", "Milestones, what works now, what comes next"),
    ("README.md", "What Suhina is, how to try and build it"),
)


@dataclass
class Doc:
    path: Path
    title: str
    fields: dict[str, str] = field(default_factory=dict)
    covers: list[str] = field(default_factory=list)
    has_front_matter: bool = False

    @property
    def rel(self) -> str:
        return self.path.relative_to(ROOT).as_posix()


def parse(path: Path) -> Doc:
    lines = path.read_text(encoding="utf-8").splitlines()
    doc = Doc(path=path, title=path.stem)
    body = lines
    if lines and lines[0].strip() == "---":
        try:
            end = lines.index("---", 1)
        except ValueError:
            end = -1
        if end > 0:
            doc.has_front_matter = True
            key = None
            for line in lines[1:end]:
                if line.startswith("  - ") or line.startswith("- "):
                    if key == "covers":
                        doc.covers.append(line.split("- ", 1)[1].strip())
                elif ":" in line:
                    key, value = (part.strip() for part in line.split(":", 1))
                    if value:
                        doc.fields[key] = value
            body = lines[end + 1 :]
    for line in body:
        if line.startswith("# "):
            doc.title = line[2:].strip()
            break
    return doc


def documents() -> list[Doc]:
    generated = {path for path, _ in GENERATED}
    paths = sorted(p for p in DOCS.rglob("*.md") if p not in generated)
    return [parse(p) for p in paths]


def tracked_files() -> list[str]:
    out = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True)
    return out.stdout.splitlines()


def matches(pattern: str, path: str) -> bool:
    return path == pattern or fnmatch.fnmatch(path, pattern) or path.startswith(pattern.rstrip("/") + "/")


def render_index(docs: list[Doc]) -> str:
    out = [
        "# Documentation index",
        "",
        "Generated by `python scripts/docs.py index` from the front matter of each document, on `main`",
        "after every merge; do not edit by hand. Find a document by what you are about to do",
        "(*Read when*), or by the code you are changing (*Code and its documents*).",
        "",
        "## Start here",
        "",
        "| Document | What it says |",
        "|---|---|",
    ]
    for rel, what in ROOT_DOCS:
        out.append(f"| [`{rel}`](../{rel}) | {what} |")
    for path, what in GENERATED[1:]:
        link = path.relative_to(DOCS).as_posix()
        out.append(f"| [`docs/{link}`]({link}) | {what} (generated) |")
    out += ["", "## Documents", "", "| Document | Type | Status | Read when | What it says |", "|---|---|---|---|---|"]
    for d in sorted(docs, key=lambda d: (TYPES.index(d.fields["type"]) if d.fields.get("type") in TYPES else 99, d.rel)):
        link = d.path.relative_to(DOCS).as_posix()
        f = d.fields
        out.append(
            f"| [{d.title}]({link}) | {f.get('type', '')} | {f.get('status', '')} "
            f"| {f.get('read_when', '')} | {f.get('description', '')} |"
        )
    covered: dict[str, list[Doc]] = {}
    for d in docs:
        for pattern in d.covers:
            covered.setdefault(pattern, []).append(d)
    out += ["", "## Code and its documents", "", "Change this code, and check these documents.", ""]
    out += ["| Code | Documents |", "|---|---|"]
    for pattern in sorted(covered):
        links = ", ".join(f"[{d.title}]({d.path.relative_to(DOCS).as_posix()})" for d in covered[pattern])
        out.append(f"| `{pattern}` | {links} |")
    return "\n".join(out) + "\n"


def annotate(level: str, doc: Doc | None, code: str, message: str) -> None:
    where = f" file={doc.rel}" if doc else ""
    print(f"::{level}{where}::{code}: {message}")


LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)|!\[[^\]]*\]\(([^)\s]+)\)")
FENCE = re.compile(r"^\s*(```|~~~)")


def slug(heading: str) -> str:
    """The anchor GitHub gives a heading: its text, lowercase, without punctuation, spaces as -."""
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", heading)  # a link's text only
    text = text.replace("`", "").replace("*", "").strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def anchors(path: Path) -> set[str]:
    found: set[str] = set()
    seen: dict[str, int] = {}
    fenced = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if FENCE.match(line):
            fenced = not fenced
            continue
        if fenced or not line.startswith("#"):
            continue
        heading = line.lstrip("#").strip()
        base = slug(heading)
        n = seen.get(base, 0)
        seen[base] = n + 1
        found.add(base if n == 0 else f"{base}-{n}")
    return found


def links(path: Path):
    """(line number, target) of every relative link in a Markdown file, outside code blocks."""
    fenced = False
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if FENCE.match(line):
            fenced = not fenced
            continue
        if fenced:
            continue
        line = re.sub(r"`[^`]*`", "", line)  # code spans are not links
        for match in LINK.finditer(line):
            target = match.group(1) or match.group(2)
            if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I):  # https:, mailto: and the like
                continue
            yield number, target


def check_links(files: list[str]) -> int:
    """Relative links in the tracked Markdown files point at a tracked file or folder, and an
    #anchor at a heading in it."""
    tracked = set(files)
    folders = {parent.as_posix() for f in files for parent in Path(f).parents}
    cache: dict[Path, set[str]] = {}
    errors = 0
    for rel in files:
        if not rel.endswith(".md") or rel.startswith("third_party/"):
            continue
        source = ROOT / rel
        if not source.exists():
            continue
        for number, target in links(source):
            file_part, _, anchor = target.partition("#")
            dest = source if not file_part else (source.parent / file_part).resolve()
            try:
                dest_rel = dest.relative_to(ROOT).as_posix()
            except ValueError:
                print(f"::error file={rel},line={number}::BROKEN_LINK: {target} leaves the repository")
                errors += 1
                continue
            if file_part and dest_rel not in tracked and dest_rel.rstrip("/") not in folders:
                print(f"::error file={rel},line={number}::BROKEN_LINK: {target}: no such file in the repository")
                errors += 1
                continue
            if anchor and dest.suffix == ".md":
                if dest not in cache:
                    cache[dest] = anchors(dest)
                if anchor.lower() not in cache[dest]:
                    print(f"::error file={rel},line={number}::BROKEN_ANCHOR: {target}: no heading with that anchor")
                    errors += 1
    return errors


def check() -> int:
    docs = documents()
    files = tracked_files()
    errors = 0
    for d in docs:
        if not d.has_front_matter:
            annotate("error", d, "NO_FRONT_MATTER", "starts without a --- front matter block")
            errors += 1
            continue
        missing = [k for k in REQUIRED if not d.fields.get(k)]
        if missing:
            annotate("error", d, "MISSING_FIELDS", "front matter lacks " + ", ".join(missing))
            errors += 1
        if d.fields.get("type") and d.fields["type"] not in TYPES:
            annotate("error", d, "BAD_TYPE", f"type {d.fields['type']!r} is not one of {', '.join(TYPES)}")
            errors += 1
        if d.fields.get("status") and d.fields["status"] not in STATUSES:
            annotate("error", d, "BAD_STATUS", f"status {d.fields['status']!r} is not one of {', '.join(STATUSES)}")
            errors += 1
        if verified := d.fields.get("last_verified"):
            try:
                age = (date.today() - date.fromisoformat(verified)).days
            except ValueError:
                annotate("error", d, "BAD_DATE", f"last_verified {verified!r} is not yyyy-mm-dd")
                errors += 1
            else:
                if age > STALE_AFTER_DAYS:
                    annotate(
                        "warning", d, "STALE",
                        f"last_verified {verified}, over {STALE_AFTER_DAYS} days ago: check it against its sources",
                    )
        for pattern in d.covers:
            if not any(matches(pattern, f) for f in files):
                annotate("error", d, "BROKEN_COVERS", f"covers {pattern!r}, which matches no file in the repository")
                errors += 1
    errors += check_links(files)
    if errors:
        print(f"{errors} problem(s) in the documentation.")
    else:
        print(f"{len(docs)} documents, front matter, covered paths and links fine.")
    return 1 if errors else 0


def impact(base: str) -> int:
    # From where this branch left `base` to the working tree, so uncommitted edits count too.
    fork = subprocess.run(["git", "merge-base", base, "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True)
    out = subprocess.run(
        ["git", "diff", "--name-only", fork.stdout.strip()], cwd=ROOT, capture_output=True, text=True, check=True
    )
    changed = out.stdout.splitlines()
    for path, _ in GENERATED:
        rel = path.relative_to(ROOT).as_posix()
        if rel in changed:
            print(f"::warning file={rel}::GENERATED: {rel} is regenerated on main after every merge; leave it out of the change so it does not conflict")
    for rel in changed:
        if not (rel.startswith("docs/") and rel.endswith(".md")):
            continue
        diff = subprocess.run(
            ["git", "diff", "-U0", fork.stdout.strip(), "--", rel], cwd=ROOT, capture_output=True, text=True, check=False
        ).stdout
        if re.search(r"^\+last_verified:", diff, re.M) and re.search(r"^-last_verified:", diff, re.M):
            print(f"::notice file={rel}::LAST_VERIFIED: last_verified is stamped on main after the merge for every document a pull request changes; leaving the line alone keeps two pull requests from conflicting on it")
    docs = documents()
    # How many documents cover each changed file: a hub alone only gets the documents listed.
    covering = {f: sum(1 for d in docs if any(matches(p, f) for p in d.covers)) for f in changed}
    behind = []
    for d in docs:
        if d.rel in changed:
            continue
        hits = sorted({f for f in changed for p in d.covers if matches(p, f)})
        if not hits:
            continue
        behind.append(d)
        if all(covering[f] >= HUB_DOCS for f in hits):
            annotate(
                "notice", d, "MAYBE_BEHIND",
                "covers a shared file this change touches (" + ", ".join(hits) + "); check it if the change is about what it describes",
            )
        else:
            annotate("warning", d, "DOCS_BEHIND", "describes changed code that this change does not update it for: " + ", ".join(hits))
    if not behind:
        print("Every document that covers the changed code was updated, or none covers it.")
    return 0


def git(*args: str) -> str:
    out = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=False)
    if out.returncode != 0:
        raise SystemExit(f"git {' '.join(args)}: {out.stderr.strip()}")
    return out.stdout


def last_changed(rel: str) -> str | None:
    """The day (yyyy-mm-dd) of the last commit since STAMP_FROM that changed `rel`, not counting
    the commits STAMP_SKIP matches; None when there is none."""
    skip = [f"--grep={pattern}" for pattern in STAMP_SKIP]
    day = git("log", "-1", "--format=%cs", f"--since={STAMP_FROM}T00:00:00Z", "--invert-grep",
              "--extended-regexp", *skip, "--", rel).strip()
    return day or None


def stamped(text: str, day: str) -> str | None:
    """`text` with last_verified in its front matter set to `day`, or None when it would not move
    forward (already that day or later, no front matter, no valid date)."""
    lines = text.split("\n")
    if not lines or lines[0].strip() != "---":
        return None
    for i, line in enumerate(lines[1:], start=1):
        if line.strip() == "---":
            return None
        match = LAST_VERIFIED.match(line)
        if not match:
            continue
        try:
            current = date.fromisoformat(match.group(1))
        except ValueError:
            return None
        if current >= date.fromisoformat(day):
            return None
        lines[i] = f"last_verified: {day}"
        return "\n".join(lines)
    return None


def stamp() -> int:
    if git("rev-parse", "--is-shallow-repository").strip() == "true":
        # A shallow clone's oldest commit seems to add every file, so every document would move.
        raise SystemExit("stamp needs the whole history: fetch it first (git fetch --unshallow)")
    moved = 0
    for d in documents():
        day = last_changed(d.rel)
        if day is None:
            continue
        text = d.path.read_text(encoding="utf-8")
        new = stamped(text, day)
        if new is None:
            continue
        d.path.write_text(new, encoding="utf-8", newline="\n")
        print(f"{d.rel}: last_verified {day}")
        moved += 1
    print(f"{moved} document(s) stamped." if moved else "Every last_verified is current.")
    return 0


def main(argv: list[str]) -> int:
    command = argv[1] if len(argv) > 1 else ""
    if command == "index":
        text = render_index(documents())
        if "--write" not in argv[2:]:
            sys.stdout.write(text)
            return 0
        INDEX.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {INDEX.relative_to(ROOT).as_posix()}")
        return 0
    if command == "check":
        return check()
    if command == "impact":
        return impact(argv[2] if len(argv) > 2 else "origin/main")
    if command == "stamp":
        return stamp()
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
