# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Check `scripts/pr_description.py`, the reminder `.github/workflows/pr-description.yml` runs.

    python scripts/pr_description_test.py
"""

from __future__ import annotations

import sys

from pr_description import TEMPLATE, missing, sections

WANTED = ["What and why", "Where the knowledge comes from", "How to check it", "How I tested it", "Checklist"]
FULL = "\n\n".join(f"## {w}\n\ntext" for w in WANTED)


def run() -> int:
    cases = [
        ("the template's own headings are read", sections(TEMPLATE.read_text(encoding="utf-8")) == WANTED),
        ("a full description misses nothing", missing(FULL, WANTED) == []),
        ("an empty description misses everything", missing("", WANTED) == WANTED),
        (
            "a missing section is named",
            missing(FULL.replace("## How I tested it", "How I tested it"), WANTED) == ["How I tested it"],
        ),
        ("case, spacing and CRLF do not matter", missing(FULL.lower().replace("\n", "\r\n").replace("## ", "##  "), WANTED) == []),
        ("a heading takes no text from the next line", missing(FULL.replace("## Checklist", "##\nChecklist"), WANTED) == ["Checklist"]),
        ("a ### heading is not a section", missing(FULL.replace("## Checklist", "### Checklist"), WANTED) == ["Checklist"]),
    ]
    failed = 0
    for name, ok in cases:
        failed += not ok
        print(f"{'PASS' if ok else 'FAIL'}  {name}")
    print(f"{len(cases) - failed}/{len(cases)} cases answered as intended")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(run())
