# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Check a pull request's description against the sections of the pull request template.

    python scripts/pr_description.py            # reads the description from $PR_BODY

The template is `.github/pull_request_template.md`; its `## ` headings are the sections every
description has (a change nobody can notice still says so in one line under *How to check it*).
Prints a GitHub warning annotation naming the sections that are missing and always exits 0: the
check only reminds, so a contributor's pull request is never held up by its description
(`.github/workflows/pr-description.yml`).
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

TEMPLATE = Path(__file__).resolve().parent.parent / ".github" / "pull_request_template.md"
HEADING = re.compile(r"^##\s+(.+?)\s*$", re.MULTILINE)


def sections(template: str) -> list[str]:
    """The `## ` headings of the template, in order."""
    return [m.group(1) for m in HEADING.finditer(template)]


def missing(body: str, wanted: list[str]) -> list[str]:
    """The headings in `wanted` that `body` has no `##` heading for (case and spacing ignored)."""
    have = {" ".join(h.lower().split()) for h in HEADING.findall(body.replace("\r\n", "\n"))}
    return [w for w in wanted if " ".join(w.lower().split()) not in have]


def main() -> int:
    body = os.environ.get("PR_BODY", "")
    wanted = sections(TEMPLATE.read_text(encoding="utf-8"))
    gone = missing(body, wanted)
    if not gone:
        print("The description has every section of the pull request template.")
    else:
        names = ", ".join(f"`{g}`" for g in gone)
        # A warning annotation, never a failure (see the docstring).
        print(
            f"::warning title=Pull request description::Missing the template's sections {names}"
            " (.github/pull_request_template.md; one line is enough where nothing applies)."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
