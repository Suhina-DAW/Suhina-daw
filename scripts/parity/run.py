# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Run the whole parity measurement, from a manual PDF you own to the report and its figures.

    python scripts/parity/run.py --pdf "C:/path/to/your/live12-manual.pdf"

Everything read from the manual goes to target/parity/ (gitignored); the report and the two
figures are written into docs/research/. See docs/research/parity-live-12.md and
CONTRIBUTING.md > Clean-room: the manual is public documentation and an allowed source, but the
manual itself is never committed and never read from the repository.
"""

import argparse
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]


def run(script: str, *extra: str) -> int:
    cmd = [sys.executable, str(HERE / script), *extra]
    print(f"\n$ {' '.join(cmd)}", flush=True)
    return subprocess.call(cmd)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", required=True, help="a reference manual PDF you own")
    parser.add_argument("--dir", default=str(REPO / "target" / "parity"))
    parser.add_argument("--out", default=str(REPO / "docs" / "research"))
    args = parser.parse_args()

    steps = [
        ("extract.py", "--pdf", args.pdf, "--out", args.dir),
        ("headings.py", "--dir", args.dir),
        ("score.py", "--dir", args.dir),
        ("charts.py", "--out", args.out),
        ("update_report.py",),
    ]
    for script, *extra in steps:
        code = run(script, *extra)
        if code != 0:
            print(f"{script} failed with {code}")
            return code
    print("\ndone: docs/research/parity-live-12.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
