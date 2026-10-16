"""Check `scripts/branch_name.py`, the branch name reminder `.github/workflows/branch-name.yml` runs.

    python scripts/branch_name_test.py
"""

from __future__ import annotations

import sys

from branch_name import problem

GOOD = [
    "feature/piano-roll-workflow",
    "fix/short-panes",
    "docs/adr-0020-latency",
    "chore/branch-names",
    "feature/a-b-c-d-e",
]

BAD = [
    "claude/merge-batch-4",
    "main",
    "feature/recording",
    "feature/a-b-c-d-e-f",
    "feature/Piano-Roll",
    "fix/short_panes",
    "fix/short--panes",
    "fix/-short-panes",
    "chore/",
    "feature/piano-roll/workflow",
]


def run() -> int:
    failed = 0
    for branch in GOOD:
        found = problem(branch)
        ok = found is None
        failed += not ok
        print(f"{'PASS' if ok else 'FAIL'}  {branch} is accepted" + ("" if ok else f": {found}"))
    for branch in BAD:
        ok = problem(branch) is not None
        failed += not ok
        print(f"{'PASS' if ok else 'FAIL'}  {branch} gets a warning")
    total = len(GOOD) + len(BAD)
    print(f"{total - failed}/{total} branch names answered as intended")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(run())
