"""Tests for discord_merged.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import unittest

import discord_api
import discord_merged as dm

FORUM = "500"


def thread(tid, name, parent=FORUM, archived=False, stamp=None):
    return {
        "id": tid,
        "name": name,
        "parent_id": parent,
        "thread_metadata": {"archived": archived, "archive_timestamp": stamp},
    }


class Names(unittest.TestCase):
    def test_marks_after_the_number(self):
        self.assertEqual(dm.merged_name("#280 Start-up package", 280), "#280 [Merged] Start-up package")

    def test_already_marked(self):
        self.assertIsNone(dm.merged_name("#280 [Merged] Start-up package", 280))

    def test_other_pull_request(self):
        self.assertIsNone(dm.merged_name("#2800 Something", 280))
        self.assertIsNone(dm.merged_name("#28 Something", 280))
        self.assertIsNone(dm.merged_name("Start-up #280", 280))

    def test_cut_to_the_limit(self):
        name = dm.merged_name("#280 " + "x" * 95, 280)
        self.assertEqual(len(name), discord_api.NAME_LIMIT)
        self.assertTrue(name.startswith("#280 [Merged] x"))
        self.assertTrue(name.endswith("…"))

    def test_number_only(self):
        self.assertEqual(dm.merged_name("#280 ", 280), "#280 [Merged]")


class Finding(unittest.TestCase):
    def test_only_in_the_forum(self):
        threads = [thread("1", "#7 Elsewhere", parent="999"), thread("2", "#7 Fades")]
        self.assertEqual(discord_api.find_thread(threads, 7, FORUM)["id"], "2")

    def test_prefix_needs_the_space(self):
        self.assertIsNone(discord_api.find_thread([thread("1", "#70 Fades")], 7, FORUM))

    def test_merged_number(self):
        self.assertEqual(dm.merged_number({"action": "closed", "pull_request": {"number": 7, "merged": True}}), 7)
        self.assertIsNone(dm.merged_number({"action": "closed", "pull_request": {"number": 7, "merged": False}}))
        self.assertIsNone(dm.merged_number({"action": "opened", "pull_request": {"number": 7}}))


class Fake:
    """Discord with canned answers; records the calls."""

    def __init__(self, active=(), pages=()):
        self.active = list(active)
        self.pages = list(pages)
        self.calls = []

    def call(self, method, path, body=None):
        self.calls.append((method, path, body))
        if path.endswith("/threads/active"):
            return {"threads": self.active}
        if "/threads/archived/public" in path:
            return self.pages.pop(0) if self.pages else {"threads": [], "has_more": False}
        return {}


class Flow(unittest.TestCase):
    def test_active_thread(self):
        fake = Fake(active=[thread("9", "#7 Fades")])
        found = discord_api.locate(fake, "1", FORUM, 7)
        dm.mark(fake, found, 7)
        self.assertEqual(
            fake.calls[1:],
            [
                ("PUT", "/channels/9/messages/9/reactions/%E2%9C%85/@me", None),
                ("PATCH", "/channels/9", {"archived": True, "name": "#7 [Merged] Fades"}),
            ],
        )

    def test_archived_thread_on_a_later_page(self):
        first = {"threads": [thread("3", "#8 Other", archived=True, stamp="2026-10-01T00:00:00+00:00")], "has_more": True}
        second = {"threads": [thread("9", "#7 Fades", archived=True)], "has_more": False}
        fake = Fake(pages=[first, second])
        found = discord_api.locate(fake, "1", FORUM, 7)
        self.assertEqual(found["id"], "9")
        self.assertIn("before=2026-10-01T00%3A00%3A00%2B00%3A00", fake.calls[2][1])
        fake.calls.clear()
        dm.mark(fake, found, 7)
        self.assertEqual(
            [(m, b) for m, _, b in fake.calls],
            [("PATCH", {"archived": False}), ("PUT", None), ("PATCH", {"archived": True, "name": "#7 [Merged] Fades"})],
        )

    def test_already_marked_and_archived(self):
        fake = Fake()
        dm.mark(fake, thread("9", "#7 [Merged] Fades", archived=True), 7)
        self.assertEqual(fake.calls, [])

    def test_marked_but_open_is_only_archived(self):
        fake = Fake()
        dm.mark(fake, thread("9", "#7 [Merged] Fades"), 7)
        self.assertEqual(fake.calls[-1], ("PATCH", "/channels/9", {"archived": True}))

    def test_no_thread(self):
        self.assertIsNone(discord_api.locate(Fake(active=[thread("1", "#8 Other")]), "1", FORUM, 7))


if __name__ == "__main__":
    unittest.main()
