"""Tests for discord_post.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import json
import unittest

import discord_post as dp


def pr(action, merged=False, draft=False):
    return {
        "action": action,
        "pull_request": {
            "number": 7,
            "title": "Clip fades:  shapes\nand a curve @everyone",
            "html_url": "https://github.com/o/r/pull/7",
            "user": {"login": "ada"},
            "merged": merged,
            "draft": draft,
        },
    }


class PullRequests(unittest.TestCase):
    def test_opened(self):
        channel, text = dp.post_for("pull_request_target", pr("opened"))
        self.assertEqual(channel, "events")
        self.assertEqual(
            text,
            "**Pull request #7 opened**: Clip fades: shapes and a curve @everyone (by ada)"
            " <https://github.com/o/r/pull/7>",
        )

    def test_draft(self):
        _, text = dp.post_for("pull_request_target", pr("opened", draft=True))
        self.assertIn("opened** (draft)", text)

    def test_merged_and_closed(self):
        self.assertIn("#7 merged**", dp.post_for("pull_request_target", pr("closed", merged=True))[1])
        self.assertIn("#7 closed**", dp.post_for("pull_request_target", pr("closed"))[1])

    def test_other_actions_are_not_posted(self):
        for action in ("synchronize", "labeled", "edited"):
            self.assertIsNone(dp.post_for("pull_request_target", pr(action)), action)


class Issues(unittest.TestCase):
    def issue(self, action, reason=None):
        return {
            "action": action,
            "issue": {
                "number": 3,
                "title": "Crash on open",
                "html_url": "https://github.com/o/r/issues/3",
                "user": {"login": "bob"},
                "state_reason": reason,
            },
        }

    def test_opened_and_closed(self):
        channel, text = dp.post_for("issues", self.issue("opened"))
        self.assertEqual(channel, "events")
        self.assertEqual(
            text, "**Issue #3 opened**: Crash on open (by bob) <https://github.com/o/r/issues/3>"
        )
        self.assertIn("closed (not planned)", dp.post_for("issues", self.issue("closed", "not_planned"))[1])

    def test_comments_are_not_posted(self):
        self.assertIsNone(dp.post_for("issue_comment", self.issue("created")))
        self.assertIsNone(dp.post_for("issues", self.issue("labeled")))


BODY = """Pre-alpha build.

<!-- Generated from ROADMAP.md. Edit before publishing. -->

## New and changed

### M8 UI

- Clip fades

## All changes

- Clip fades (#7)
"""


def release(body=BODY, tag="v0.1.0", draft=False, action="published"):
    return {
        "action": action,
        "release": {
            "tag_name": tag,
            "name": f"Suhina {tag[1:]}",
            "html_url": f"https://github.com/o/r/releases/tag/{tag}",
            "body": body,
            "draft": draft,
        },
    }


class Releases(unittest.TestCase):
    def test_published_with_new_and_changed(self):
        channel, text = dp.post_for("release", release())
        self.assertEqual(channel, "releases")
        self.assertEqual(
            text,
            "**Suhina 0.1.0** is out: <https://github.com/o/r/releases/tag/v0.1.0>\n\n"
            "### M8 UI\n\n- Clip fades",
        )

    def test_without_the_section_only_the_link(self):
        _, text = dp.post_for("release", release(body="Edited by hand."))
        self.assertEqual(text, "**Suhina 0.1.0** is out: <https://github.com/o/r/releases/tag/v0.1.0>")

    def test_nightly_drafts_and_other_actions_are_not_posted(self):
        self.assertIsNone(dp.post_for("release", release(tag="nightly")))
        self.assertIsNone(dp.post_for("release", release(draft=True)))
        self.assertIsNone(dp.post_for("release", release(action="created")))

    def test_long_notes_are_cut_at_a_line_with_a_link(self):
        body = "## New and changed\n\n" + "\n".join(f"- feature {i} " + "x" * 80 for i in range(100))
        _, text = dp.post_for("release", release(body=body))
        self.assertLessEqual(len(text), dp.LIMIT)
        self.assertTrue(text.endswith("…and more on the release page: <https://github.com/o/r/releases/tag/v0.1.0>"))


class Payload(unittest.TestCase):
    def test_no_mentions_and_limit(self):
        data = json.loads(dp.payload("@everyone " + "x" * 3000))
        self.assertEqual(data["allowed_mentions"], {"parse": []})
        self.assertEqual(len(data["content"]), dp.LIMIT)


if __name__ == "__main__":
    unittest.main()
