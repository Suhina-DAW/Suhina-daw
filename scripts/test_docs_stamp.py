# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for `scripts/docs.py stamp` and the last_verified notice of `impact`:
python3 -m unittest discover -s scripts -p 'test_*.py'

Each test builds a throwaway git repository with dated commits, so they need git and nothing else.
"""

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs  # noqa: E402


def front(day: str, body: str = "Text.") -> str:
    return (
        "---\n"
        "description: A document\n"
        "read_when: testing\n"
        "type: howto\n"
        "status: current\n"
        f"last_verified: {day}\n"
        "---\n\n"
        f"# A document\n\n{body}\n"
    )


class Stamp(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "docs").mkdir()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        for name, value in (("ROOT", self.root), ("DOCS", self.root / "docs"),
                            ("STAMP_FROM", "2026-10-17")):
            patcher = mock.patch.object(docs, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def git(self, *args, day=None):
        env = dict(os.environ)
        if day:
            env["GIT_AUTHOR_DATE"] = env["GIT_COMMITTER_DATE"] = f"{day}T12:00:00Z"
        return subprocess.run(["git", *args], cwd=self.root, env=env, check=True,
                              capture_output=True, text=True).stdout

    def commit(self, files: dict, message: str, day: str):
        for rel, text in files.items():
            path = self.root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message, day=day)

    def read(self, rel):
        return (self.root / rel).read_text(encoding="utf-8")

    def run_stamp(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = docs.stamp()
        return code, out.getvalue()

    def test_moves_a_changed_document_up_to_the_day_of_its_change(self):
        self.commit({"docs/a.md": front("2026-09-01"), "docs/b.md": front("2026-09-01")},
                    "Add documents", "2026-10-01")
        self.commit({"docs/a.md": front("2026-09-01", "Changed text.")}, "Change a", "2026-10-20")
        code, out = self.run_stamp()
        self.assertEqual(code, 0)
        self.assertIn("last_verified: 2026-10-20\n", self.read("docs/a.md"))
        self.assertIn("Changed text.", self.read("docs/a.md"))
        # Unchanged since STAMP_FROM: left as it is.
        self.assertIn("last_verified: 2026-09-01\n", self.read("docs/b.md"))
        self.assertIn("docs/a.md: last_verified 2026-10-20", out)

    def test_changes_only_the_date_line(self):
        self.commit({"docs/a.md": front("2026-09-01")}, "Add", "2026-10-18")
        before = self.read("docs/a.md")
        self.run_stamp()
        after = self.read("docs/a.md")
        self.assertEqual(before.replace("2026-09-01", "2026-10-18"), after)

    def test_never_moves_a_date_back(self):
        self.commit({"docs/a.md": front("2026-11-02")}, "Checked by hand later", "2026-10-18")
        self.run_stamp()
        self.assertIn("last_verified: 2026-11-02\n", self.read("docs/a.md"))

    def test_skips_the_bots_commits_and_marked_ones(self):
        self.commit({"docs/a.md": front("2026-09-01")}, "Add", "2026-10-01")
        self.commit({"docs/a.md": front("2026-09-01", "Regenerated.")},
                    "Docs: regenerate INDEX.md", "2026-10-20")
        self.commit({"docs/a.md": front("2026-09-01", "Renamed.")},
                    "Rename everything\n\nLast-verified: keep", "2026-10-21")
        code, out = self.run_stamp()
        self.assertEqual(code, 0)
        self.assertIn("last_verified: 2026-09-01\n", self.read("docs/a.md"))
        self.assertIn("Every last_verified is current.", out)

    def test_a_second_run_changes_nothing(self):
        self.commit({"docs/a.md": front("2026-09-01")}, "Add", "2026-10-18")
        self.run_stamp()
        once = self.read("docs/a.md")
        _, out = self.run_stamp()
        self.assertEqual(self.read("docs/a.md"), once)
        self.assertIn("Every last_verified is current.", out)

    def test_refuses_a_shallow_clone(self):
        self.commit({"docs/a.md": front("2026-09-01")}, "Add", "2026-10-18")
        self.commit({"docs/a.md": front("2026-09-01", "Two")}, "Two", "2026-10-19")
        shallow = self.root / "shallow"
        subprocess.run(["git", "clone", "-q", "--depth", "1", f"file://{self.root}", str(shallow)],
                       check=True, capture_output=True)
        with mock.patch.object(docs, "ROOT", shallow), mock.patch.object(docs, "DOCS", shallow / "docs"):
            with self.assertRaises(SystemExit) as caught:
                docs.stamp()
        self.assertIn("whole history", str(caught.exception.code))


class Stamped(unittest.TestCase):
    def test_only_the_front_matter_date_moves(self):
        text = front("2026-09-01", "last_verified: 2000-01-01 in the body stays.")
        new = docs.stamped(text, "2026-10-20")
        self.assertIn("last_verified: 2026-10-20\n---", new)
        self.assertIn("last_verified: 2000-01-01 in the body stays.", new)

    def test_nothing_without_front_matter_or_a_valid_date(self):
        self.assertIsNone(docs.stamped("# No front matter\n", "2026-10-20"))
        self.assertIsNone(docs.stamped(front("someday"), "2026-10-20"))
        self.assertIsNone(docs.stamped(front("2026-10-20"), "2026-10-20"))


if __name__ == "__main__":
    unittest.main()
