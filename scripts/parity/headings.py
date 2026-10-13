"""Locate chapter and section headings in the extracted manual text.

    python scripts/parity/headings.py [--dir target/parity]

Reads <dir>/manual.txt (see extract.py) and writes <dir>/headings.json: a list of
{page, kind, num, title}. The PDF prints some digits apart ("3 1. Live Instrument Reference"
is chapter 31), so every line is normalised before it is matched.
"""

import argparse
import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# The manual body starts after the table of contents; before that the same numbers appear as
# page references, which would be read as headings.
BODY_START = 23

CHAP = re.compile(r"^(\d{1,2})\s*\.\s+([A-Z][^.]{2,60})$")
SEC = re.compile(r"^(\d{1,2}\s*\.\s*\d{1,2})\s+([A-Z][^.]{2,70})$")


def squeeze(line: str) -> str:
    """Join digits the PDF separated: '1 1. MIDI Tools' is chapter 11."""
    prev = None
    while prev != line:
        prev = line
        line = re.sub(r"(?<=\d) (?=\d)", "", line)
    return line


def find_headings(pages: dict) -> list:
    headings = []
    for page in sorted(pages):
        if page < BODY_START:
            continue
        for line in pages[page].splitlines():
            text = squeeze(line.strip())
            match = CHAP.match(text)
            if match:
                headings.append(
                    {"page": page, "kind": "chapter", "num": match.group(1),
                     "title": match.group(2).strip()}
                )
                continue
            match = SEC.match(text)
            if match:
                headings.append(
                    {"page": page, "kind": "section", "num": match.group(1),
                     "title": match.group(2).strip()}
                )
    return headings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", default=str(REPO / "target" / "parity"))
    args = parser.parse_args()

    work = Path(args.dir)
    raw = (work / "manual.txt").read_text(encoding="utf-8")
    parts = re.split(r"===== PAGE (\d+) =====", raw)
    pages = {int(parts[i]): parts[i + 1] for i in range(1, len(parts), 2)}

    headings = find_headings(pages)
    (work / "headings.json").write_text(
        json.dumps(headings, ensure_ascii=False, indent=1), encoding="utf-8", newline="\n"
    )

    chapters = [h for h in headings if h["kind"] == "chapter"]
    sections = [h for h in headings if h["kind"] == "section"]
    print(f"{len(chapters)} chapters, {len(sections)} sections -> {work / 'headings.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
