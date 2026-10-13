"""Renames Loopwerk to Suhina in the working tree: text in every tracked file, then paths.

    python scripts/rename_suhina.py           # rename (Cargo.lock is rewritten like any other file)
    python scripts/rename_suhina.py --check   # list what still says loopwerk, change nothing

`Loopwerk` becomes `Suhina`, `loopwerk` `suhina` and `LOOPWERK` `SUHINA`, so crate names
(`loopwerk-engine`, `loopwerk_engine`), environment variables and binaries follow. Left alone:

- links to the repository (`github.com/juhamikael/Loopwerk`), which GitHub redirects after a move;
- `loopwerk-demo`, which names tracks and files inside self-authored Live Sets outside the repo
  (crates/als/tests/corpus.rs);
- any line that contains `rename_suhina: keep`, such as code that finds the old settings folder.

A branch made before the rename catches up like this, so that only its own conflicts come up
(RENAME is the commit on main that ran this script, PRE its parent):

    git merge PRE                                   # main before the rename; old names
    git checkout RENAME -- scripts/rename_suhina.py
    python scripts/rename_suhina.py                 # renames the branch, main's part included
    git add -A && git commit -m "Rename Loopwerk to Suhina in this branch"
    git merge -s ours RENAME                        # the rename is already in the branch
    git merge origin/main                           # what main got after the rename

A branch that already ran the script on its own starts with `git revert <its rename commit>`.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

# The checkout it runs in, so a branch can run a copy taken from main (`git show
# origin/main:scripts/rename_suhina.py > ...`) before main is merged in.
ROOT = Path(
    subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, check=True, text=True).stdout.strip()
)
KEEP_MARK = "rename_suhina: keep"
# Kept as written: each match is swapped for a placeholder before renaming and put back after.
PROTECTED = [
    re.compile(r"github\.com/juhamikael/Loopwerk", re.IGNORECASE),
    re.compile(r"loopwerk-demo"),
]
WORDS = [("Loopwerk", "Suhina"), ("loopwerk", "suhina"), ("LOOPWERK", "SUHINA")]
SELF = Path(__file__).resolve()


def tracked() -> list[Path]:
    out = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, check=True).stdout
    return [ROOT / p for p in out.decode("utf-8").split("\0") if p]


def rename_line(line: str) -> str:
    if KEEP_MARK in line:
        return line
    kept: list[str] = []

    def hold(m: re.Match[str]) -> str:
        kept.append(m.group(0))
        return f"\0{len(kept) - 1}\0"

    for pattern in PROTECTED:
        line = pattern.sub(hold, line)
    for old, new in WORDS:
        line = line.replace(old, new)
    return re.sub(r"\0(\d+)\0", lambda m: kept[int(m.group(1))], line)


def rename_text(text: str) -> str:
    return "".join(rename_line(line) for line in text.splitlines(keepends=True))


def main() -> int:
    check = "--check" in sys.argv[1:]
    left = 0
    for path in tracked():
        if path.resolve() == SELF or not path.is_file():
            continue
        try:
            text = path.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            continue
        renamed = rename_text(text)
        if renamed != text:
            left += 1
            if check:
                print(path.relative_to(ROOT).as_posix())
            else:
                path.write_bytes(renamed.encode("utf-8"))
    # Deepest paths first, so a folder moves after the files inside it.
    for path in sorted(tracked(), key=lambda p: len(p.parts), reverse=True):
        rel = path.relative_to(ROOT)
        for i in range(len(rel.parts)):
            part = rel.parts[i]
            if "loopwerk" not in part.lower():
                continue
            old = ROOT.joinpath(*rel.parts[: i + 1])
            new = old.with_name(rename_line(part))
            if not old.exists():
                continue
            left += 1
            if check:
                print(f"{old.relative_to(ROOT).as_posix()} -> {new.relative_to(ROOT).as_posix()}")
            elif not new.exists():
                subprocess.run(["git", "mv", str(old), str(new)], cwd=ROOT, check=True)
            else:
                # The folder already exists under the new name (a merge put files there): move
                # what is left one file at a time.
                for inner in sorted(old.rglob("*")):
                    if inner.is_file():
                        target = new / inner.relative_to(old)
                        target.parent.mkdir(parents=True, exist_ok=True)
                        subprocess.run(["git", "mv", str(inner), str(target)], cwd=ROOT, check=True)
    if not check:
        # Shorter names rewrap lines; formatting here makes every branch end up with the same text.
        subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=False)
        # Cargo.lock lists packages by name, so renamed ones move; let cargo write the new order.
        subprocess.run(["cargo", "metadata", "--format-version", "1", "--offline"], cwd=ROOT, check=False, capture_output=True)
    if check:
        print(f"{left} files or paths still say loopwerk" if left else "nothing says loopwerk")
        return 1 if left else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
