"""Tests for scripts/parity/: python3 -m unittest discover -s scripts -p 'test_*.py'

None of them needs a copy of the manual or matplotlib: they cover the scoring, the heading
finder on synthetic text, and update_report.py's marked sections on temporary files.
"""

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent / "parity"))

import headings  # noqa: E402
import score  # noqa: E402
import update_report  # noqa: E402


def row(num, pages, coverage):
    return (num, f"Chapter {num}", pages, coverage, "status", "note")


class Weighted(unittest.TestCase):
    def test_weights_coverage_by_pages(self):
        cov, pages = score.weighted([row(1, 10, 1.0), row(2, 30, 0.0)])
        self.assertEqual(pages, 40)
        self.assertAlmostEqual(cov, 0.25)

    def test_ignores_chapters_without_a_coverage(self):
        cov, pages = score.weighted([row(1, 10, 0.5), row(2, 90, None)])
        self.assertEqual(pages, 10)
        self.assertAlmostEqual(cov, 0.5)

    def test_single_chapter_is_its_own_coverage(self):
        cov, pages = score.weighted([row(7, 13, 0.35)])
        self.assertEqual(pages, 13)
        self.assertAlmostEqual(cov, 0.35)


class Chapters(unittest.TestCase):
    def test_chapters_are_numbered_one_to_the_manual_count(self):
        self.assertEqual([r[0] for r in score.CHAPTERS],
                         list(range(1, score.MANUAL_CHAPTERS + 1)))

    def test_coverage_is_a_share(self):
        for num, _, pages, cov, _, _ in score.CHAPTERS:
            self.assertGreater(pages, 0, num)
            if cov is not None:
                self.assertTrue(0.0 <= cov <= 1.0, num)

    def test_areas_and_scope_name_real_chapters(self):
        known = {r[0] for r in score.CHAPTERS}
        for name, nums in score.AREAS.items():
            self.assertTrue(set(nums) <= known, name)
        self.assertTrue(score.OUT_OF_SCOPE <= known)

    def test_out_of_scope_is_what_adr_0016_lists(self):
        # Video, Max for Live and its devices, Push 1 and Push 2.
        self.assertEqual(score.OUT_OF_SCOPE, {28, 32, 33, 35, 36})


class Squeeze(unittest.TestCase):
    def test_joins_digits_the_pdf_printed_apart(self):
        self.assertEqual(headings.squeeze("3 1. Live Instrument Reference"),
                         "31. Live Instrument Reference")
        self.assertEqual(headings.squeeze("1 2 3"), "123")

    def test_leaves_other_text_alone(self):
        self.assertEqual(headings.squeeze("31. Live Instrument Reference"),
                         "31. Live Instrument Reference")
        self.assertEqual(headings.squeeze("Chapter 4 of the manual"), "Chapter 4 of the manual")
        self.assertEqual(headings.squeeze(""), "")


class FindHeadings(unittest.TestCase):
    def test_finds_chapters_and_sections_in_the_body(self):
        body = headings.BODY_START
        pages = {
            body: "3 1. Synth Reference\nSome running text. It ends here.\n",
            body + 1: "  31.1 First Section  \n3 1. 2 Second Section\n",
        }
        found = headings.find_headings(pages)
        self.assertEqual(found, [
            {"page": body, "kind": "chapter", "num": "31", "title": "Synth Reference"},
            {"page": body + 1, "kind": "section", "num": "31.1", "title": "First Section"},
            {"page": body + 1, "kind": "section", "num": "31. 2", "title": "Second Section"},
        ])

    def test_skips_the_table_of_contents(self):
        pages = {headings.BODY_START - 1: "4. Working With Things\n4.1 Something Else\n"}
        self.assertEqual(headings.find_headings(pages), [])

    def test_ignores_lines_that_are_not_headings(self):
        pages = {headings.BODY_START: "\n".join([
            "4. lowercase title",          # a title starts with a capital
            "4. Ends with a full stop.",   # no full stop in a title
            "123. Too Many Digits",        # chapters have one or two digits
            "Page 4",
            "4.1",
        ])}
        self.assertEqual(headings.find_headings(pages), [])

    def test_section_numbers_count_toward_their_chapter(self):
        # section_metrics() reads the chapter from a section number the same way.
        found = headings.find_headings({headings.BODY_START: "2 9. 1 2 Some Effect\n"})
        self.assertEqual(len(found), 1)
        self.assertEqual(int(headings.squeeze(found[0]["num"]).split(".")[0]), 29)


class UpdateReport(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.doc = Path(self.tmp.name) / "report.md"
        # The figures and the headline read target/parity/ when it exists; point them at an empty
        # directory so the test does not depend on a local measurement.
        patcher = mock.patch.object(update_report, "REPO", Path(self.tmp.name))
        patcher.start()
        self.addCleanup(patcher.stop)
        self.keys = list(update_report.values(self.devices()).keys())

    @staticmethod
    def devices():
        import json
        return json.loads((update_report.HERE / "devices.json").read_text(encoding="utf-8"))

    def stale_doc(self, extra=""):
        parts = ["Prose before the markers stays as it is.\n"]
        for key in self.keys:
            parts.append(f"\nText about {key}.\n\n<!-- parity:begin {key} -->stale<!-- parity:end -->\n")
        parts.append(extra)
        parts.append("\nProse after the markers stays too.\n")
        return "".join(parts)

    def run_main(self, *args):
        argv = ["update_report.py", "--doc", str(self.doc), *args]
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "argv", argv), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = update_report.main()
        return code, err.getvalue()

    def test_check_fails_on_stale_sections_and_changes_nothing(self):
        text = self.stale_doc()
        self.doc.write_text(text, encoding="utf-8")
        code, err = self.run_main("--check")
        self.assertEqual(code, 1)
        self.assertIn("out of date", err)
        self.assertEqual(self.doc.read_text(encoding="utf-8"), text)

    def test_update_replaces_only_the_marked_sections(self):
        self.doc.write_text(self.stale_doc(), encoding="utf-8")
        code, _ = self.run_main()
        self.assertEqual(code, 0)
        updated = self.doc.read_text(encoding="utf-8")
        self.assertNotIn("stale", updated)
        self.assertTrue(updated.startswith("Prose before the markers stays as it is.\n"))
        self.assertTrue(updated.endswith("\nProse after the markers stays too.\n"))
        for key in self.keys:
            self.assertIn(f"\nText about {key}.\n", updated)
            self.assertEqual(updated.count(f"<!-- parity:begin {key} -->"), 1)
        # A table owns whole lines: the marker, a newline, the table, a newline, the end marker.
        self.assertIn("<!-- parity:begin area-table -->\n| Area |", updated)
        # The title is inline, so it stays on the marker's line.
        self.assertIn(f"<!-- parity:begin title --># Parity with Ableton Live 12 (status check, "
                      f"{score.MEASURED_AT})<!-- parity:end -->", updated)

    def test_check_passes_once_updated_and_update_is_stable(self):
        self.doc.write_text(self.stale_doc(), encoding="utf-8")
        self.run_main()
        once = self.doc.read_text(encoding="utf-8")
        code, _ = self.run_main("--check")
        self.assertEqual(code, 0)
        self.run_main()
        self.assertEqual(self.doc.read_text(encoding="utf-8"), once)

    def test_unknown_marker_is_refused(self):
        self.doc.write_text(self.stale_doc(
            "\n<!-- parity:begin no-such-key -->x<!-- parity:end -->\n"), encoding="utf-8")
        with self.assertRaises(SystemExit) as caught:
            self.run_main("--check")
        self.assertIn("no-such-key", str(caught.exception.code))

    def test_duplicate_marker_is_refused(self):
        self.doc.write_text(self.stale_doc(
            "\n<!-- parity:begin area-table -->x<!-- parity:end -->\n"), encoding="utf-8")
        with self.assertRaises(SystemExit) as caught:
            self.run_main()
        self.assertIn("more than once: area-table", str(caught.exception.code))

    def test_missing_marker_is_refused(self):
        text = self.stale_doc().replace(
            "<!-- parity:begin gap-table -->stale<!-- parity:end -->", "")
        self.doc.write_text(text, encoding="utf-8")
        with self.assertRaises(SystemExit) as caught:
            self.run_main()
        self.assertIn("no marker for gap-table", str(caught.exception.code))
        self.assertEqual(self.doc.read_text(encoding="utf-8"), text)


if __name__ == "__main__":
    unittest.main()
