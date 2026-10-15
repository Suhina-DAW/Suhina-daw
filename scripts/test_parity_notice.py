"""Tests for scripts/parity/pr_notice.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "parity"))

import pr_notice  # noqa: E402


class Notice(unittest.TestCase):
    def test_code_without_measurement_or_line_warns(self):
        message = pr_notice.notice(["crates/engine/src/lib.rs", "README.md"], "Closes #1")
        self.assertIsNotNone(message)
        self.assertIn("crates/engine/src/lib.rs", message)
        self.assertNotIn("README.md", message)

    def test_each_code_directory_counts(self):
        for path in ["crates/a.rs", "devices/limiter/src/lib.rs", "apps/suhina/src/main.rs"]:
            self.assertIsNotNone(pr_notice.notice([path], ""), path)

    def test_no_code_is_quiet(self):
        files = ["docs/architecture.md", "scripts/docs.py", ".github/workflows/ci.yml"]
        self.assertIsNone(pr_notice.notice(files, ""))
        self.assertIsNone(pr_notice.notice([], ""))

    def test_updated_measurement_is_quiet(self):
        for measurement in pr_notice.MEASUREMENT:
            self.assertIsNone(pr_notice.notice(["crates/a.rs", measurement], ""))

    def test_other_parity_files_do_not_count(self):
        self.assertIsNotNone(pr_notice.notice(["crates/a.rs", "scripts/parity/charts.py"], ""))

    def test_no_change_line_is_quiet_in_any_case(self):
        for body in ["Parity: no change, a refactor.", "x\nparity: NO CHANGE\n"]:
            self.assertIsNone(pr_notice.notice(["apps/x.rs"], body))

    def test_a_path_that_only_contains_the_name_does_not_count(self):
        self.assertIsNone(pr_notice.notice(["docs/crates/a.md", "xapps/b"], ""))

    def test_long_lists_are_shortened(self):
        files = [f"crates/f{i}.rs" for i in range(8)]
        self.assertIn("and 3 more", pr_notice.notice(files, ""))


class Main(unittest.TestCase):
    def run_main(self, files: str, body: str) -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as tmp:
            listing = Path(tmp) / "files.txt"
            listing.write_text(files, "utf-8")
            description = Path(tmp) / "body.txt"
            description.write_text(body, "utf-8")
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                code = pr_notice.main(["--files", str(listing), "--body-file", str(description)])
        return code, out.getvalue()

    def test_warning_never_fails(self):
        code, out = self.run_main("crates/a.rs\n", "")
        self.assertEqual(code, 0)
        self.assertIn("::warning", out)

    def test_quiet_case(self):
        code, out = self.run_main("crates/a.rs\n", "Parity: no change (tests only)")
        self.assertEqual(code, 0)
        self.assertNotIn("::warning", out)

    def test_missing_body_file_is_an_empty_description(self):
        out = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            listing = Path(tmp) / "files.txt"
            listing.write_text("devices/a.rs\n", "utf-8")
            with contextlib.redirect_stdout(out):
                code = pr_notice.main(
                    ["--files", str(listing), "--body-file", str(Path(tmp) / "missing")]
                )
        self.assertEqual(code, 0)
        self.assertIn("::warning", out.getvalue())


if __name__ == "__main__":
    unittest.main()
