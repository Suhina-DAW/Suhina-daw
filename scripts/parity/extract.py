"""Extract a reference manual PDF to plain text, one page per marker, for the parity measurement.

The manual itself is never committed and never read from the repository: point --pdf at a copy
you own. Everything this writes goes to target/parity/ (gitignored) by default.

    python scripts/parity/extract.py --pdf "C:/path/to/live12-manual-en.pdf"

Writes <dir>/manual.txt (with "===== PAGE n =====" markers) and <dir>/toc.json.
"""

import argparse
import json
import os
from pathlib import Path

import pypdf

REPO = Path(__file__).resolve().parents[2]


def default_dir() -> Path:
    return REPO / "target" / "parity"


def walk_outline(reader, items, depth=0, acc=None):
    """Flatten a pypdf outline; a manual without bookmarks yields an empty list."""
    if acc is None:
        acc = []
    for item in items:
        if isinstance(item, list):
            walk_outline(reader, item, depth + 1, acc)
            continue
        try:
            page = reader.get_destination_page_number(item)
        except Exception:  # a broken destination is not worth failing the run over
            page = -1
        acc.append({"depth": depth, "title": item.title, "page": page})
    return acc


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", required=True, help="a reference manual PDF you own")
    parser.add_argument("--out", default=None, help=f"output directory (default {default_dir()})")
    args = parser.parse_args()

    src = Path(args.pdf).expanduser()
    if not src.is_file():
        print(f"no such file: {src}")
        return 2

    out = Path(args.out).expanduser() if args.out else default_dir()
    out.mkdir(parents=True, exist_ok=True)

    reader = pypdf.PdfReader(str(src))

    try:
        toc = walk_outline(reader, reader.outline)
    except Exception as exc:  # noqa: BLE001 - a missing outline is fine, we read the headings
        toc = [{"error": str(exc)}]
    (out / "toc.json").write_text(
        json.dumps(toc, ensure_ascii=False, indent=1), encoding="utf-8", newline="\n"
    )

    pages = []
    total = len(reader.pages)
    for i, page in enumerate(reader.pages):
        try:
            text = page.extract_text() or ""
        except Exception as exc:  # noqa: BLE001
            text = f"<<extract failed: {exc}>>"
        pages.append(f"\n\n===== PAGE {i + 1} =====\n{text}")
        if i % 50 == 0:
            print(f"page {i + 1}/{total}", flush=True)

    (out / "manual.txt").write_text("".join(pages), encoding="utf-8", newline="\n")
    print(f"wrote {out / 'manual.txt'} ({total} pages)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
