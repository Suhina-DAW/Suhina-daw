#!/usr/bin/env python3
"""Tells whether a push to a pull request only merged main into it, for
.github/workflows/test-build.yml (docs/releasing.md › Test builds).

    branch_update.py BEFORE HEAD MAIN

BEFORE is the pull request's head before the push, HEAD its head after it and MAIN the main branch
(a ref or commit). Exits 0 when HEAD only brings main into BEFORE, so the pull request's own changes
are what they were and the test build of BEFORE is still the one to try; 1 when HEAD has to be
built; 2 when git could not answer (a commit that was not fetched), which the workflow treats as 1.

HEAD only merges main when it is a merge commit whose first parent is BEFORE, whose second parent is
on main, and whose own diff equals BEFORE's: the diff from the second parent to HEAD is the same
patch (git patch-id, which ignores line numbers) as the diff from where BEFORE left main to BEFORE.
A conflict resolved by hand, or main changing lines next to the pull request's own, changes that
patch, and the pull request is built again. Standard library only.
"""

from __future__ import annotations

import subprocess
import sys


class GitError(Exception):
    pass


def git(*args: str, cwd: str | None = None, stdin: bytes | None = None) -> bytes:
    result = subprocess.run(
        ["git", *args], cwd=cwd, input=stdin, capture_output=True, check=False
    )
    if result.returncode != 0:
        raise GitError(f"git {' '.join(args)}: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def commit(rev: str, cwd: str | None = None) -> str:
    return git("rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}", cwd=cwd).decode().strip()


def patch_id(diff: bytes, cwd: str | None = None) -> str:
    """The stable patch id of a diff; empty for an empty diff."""
    if not diff.strip():
        return ""
    out = git("patch-id", "--stable", cwd=cwd, stdin=diff).decode().split()
    return out[0] if out else ""


def own_diff(base: str, tip: str, cwd: str | None = None) -> bytes:
    return git("diff", "--binary", "--no-renames", base, tip, cwd=cwd)


def only_merges_main(before: str, head: str, main: str, cwd: str | None = None) -> tuple[bool, str]:
    """(True, why) when `head` only merges `main` into `before`; (False, why) otherwise."""
    if not before or set(before) == {"0"}:
        return False, "no previous head"
    before, head, main = commit(before, cwd), commit(head, cwd), commit(main, cwd)
    parents = git("show", "-s", "--format=%P", head, cwd=cwd).decode().split()
    if len(parents) != 2:
        return False, f"{head[:7]} is not a merge commit"
    first, second = parents
    if first != before:
        return False, f"{head[:7]} does not continue the previous head {before[:7]}"
    on_main = subprocess.run(
        ["git", "merge-base", "--is-ancestor", second, main], cwd=cwd, capture_output=True, check=False
    )
    if on_main.returncode == 1:
        return False, f"{head[:7]} merges {second[:7]}, which is not on main"
    if on_main.returncode != 0:
        raise GitError(on_main.stderr.decode(errors="replace").strip())
    fork = git("merge-base", second, before, cwd=cwd).decode().strip()
    old = patch_id(own_diff(fork, before, cwd), cwd)
    new = patch_id(own_diff(second, head, cwd), cwd)
    if old != new:
        return False, f"{head[:7]} changes the pull request's own diff (a resolved conflict, or main changed lines next to it)"
    return True, f"{head[:7]} only merges main ({second[:7]}) into {before[:7]}"


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    try:
        only, why = only_merges_main(argv[1], argv[2], argv[3])
    except GitError as error:
        print(f"Cannot tell: {error}")
        return 2
    print(why)
    return 0 if only else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
