"""Tests for discord_test_build_backfill.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import datetime
import unittest
from unittest import mock

import discord_api
import discord_test_build as dt
import discord_test_build_backfill as bf

RUN = "https://github.com/o/r/actions/runs/5"
# A comment as test-build-links.yml writes it.
COMMENT = "\n".join([
    "<!-- suhina-test-build -->",
    "### Test build",
    "",
    f"Commit `e520086` merged with main, kept until 2026-10-22. [The run]({RUN}).",
    "",
    "| Platform | Download | Then |",
    "|---|---|---|",
    f"| Windows x86-64 | [suhina-pr7-windows-x86_64]({RUN}/artifacts/1) (15 MB) | unzip and run `suhina.exe` |",
    "| macOS 11+, Apple silicon and Intel | failure ([log](https://github.com/o/r/actions/runs/5/job/9)) | |",
    "| Linux x86-64, needs libasound2 | not built | |",
    "",
    "Downloading needs a GitHub sign-in. The builds are unsigned: SmartScreen (More info › Run anyway) and Gatekeeper (right-click › Open) warn about them.",
])
CONTRIBUTOR = COMMENT + "\n\n> **A contributor's build.** A maintainer added `test-build` after reading `e520086`; later pushes are not built until the label is added again. Read the diff before you run it."
PR = {"number": 7, "title": "Clip fades", "html_url": "https://github.com/o/r/pull/7"}
TODAY = datetime.date(2026, 10, 15)


def comment(body=COMMENT, kind="Bot", url="https://github.com/o/r/pull/7#issuecomment-1"):
    return {"user": {"type": kind}, "body": body, "html_url": url}


class Numbers(unittest.TestCase):
    def test_list(self):
        self.assertEqual(bf.numbers("302, #303  304"), [302, 303, 304])

    def test_empty_is_all(self):
        self.assertEqual(bf.numbers("  "), [])

    def test_malformed(self):
        self.assertIsNone(bf.numbers("302 x"))
        self.assertIsNone(bf.numbers("0"))


class Reading(unittest.TestCase):
    def test_comment_to_build_data(self):
        build = bf.parse_comment(COMMENT)
        self.assertEqual(
            bf.build_data(PR, comment(), build),
            {
                "number": 7,
                "title": "Clip fades",
                "pr_url": "https://github.com/o/r/pull/7",
                "comment_url": "https://github.com/o/r/pull/7#issuecomment-1",
                "run_url": RUN,
                "commit": "e520086",
                "contributor": False,
                "rows": [
                    {"label": "Windows x86-64", "artifact": {"name": "suhina-pr7-windows-x86_64", "url": f"{RUN}/artifacts/1", "size_mb": "15"}},
                    {"label": "macOS 11+, Apple silicon and Intel", "state": "failure", "log": "https://github.com/o/r/actions/runs/5/job/9"},
                    {"label": "Linux x86-64, needs libasound2", "state": "not built", "log": None},
                ],
            },
        )
        self.assertEqual(build["expires"], "2026-10-22")

    def test_the_message_is_the_one_a_new_build_gets(self):
        data = bf.build_data(PR, comment(), bf.parse_comment(COMMENT))
        text = dt.message(data, True)
        self.assertIn("- Windows x86-64: [suhina-pr7-windows-x86_64](<https://github.com/o/r/actions/runs/5/artifacts/1>) (15 MB)", text)
        self.assertIn("- macOS 11+, Apple silicon and Intel: failure ([log](<https://github.com/o/r/actions/runs/5/job/9>))", text)
        self.assertIn("- Linux x86-64, needs libasound2: not built\n", text)

    def test_contributor(self):
        self.assertTrue(bf.parse_comment(CONTRIBUTOR)["contributor"])

    def test_no_expiry(self):
        body = COMMENT.replace(", kept until 2026-10-22", "")
        self.assertIsNone(bf.parse_comment(body)["expires"])

    def test_not_a_link_comment(self):
        self.assertIsNone(bf.parse_comment("Nice work"))
        self.assertIsNone(bf.parse_comment(bf.MARKER + "\nedited by hand"))

    def test_only_the_bots_latest_comment(self):
        older = comment(url="old")
        newer = comment(url="new")
        self.assertIs(bf.link_comment([older, comment(kind="User"), newer, comment("hi")]), newer)
        self.assertIsNone(bf.link_comment([comment(kind="User")]))

    def test_expired(self):
        build = bf.parse_comment(COMMENT)
        self.assertFalse(bf.expired(build, datetime.date(2026, 10, 22)))
        self.assertTrue(bf.expired(build, datetime.date(2026, 10, 23)))


class Backfill(unittest.TestCase):
    def run_backfill(self, comments, thread=None, history=(), error=None):
        posts = []

        def send(method, url, label, body=None, headers=None):
            posts.append((url, body))
            if error:
                raise error
            return {"channel_id": "77"}

        discord = mock.Mock()
        discord.call.return_value = list(history)
        with mock.patch.object(bf, "locate", return_value=thread), mock.patch.object(dt, "send", send):
            outcomes = bf.backfill([PR], lambda n: comments, discord, "https://hook", "1", "500", TODAY)
        return outcomes[7], posts

    def test_opens_the_thread(self):
        outcome, [(url, body)] = self.run_backfill([comment()])
        self.assertEqual(outcome, "opened")
        self.assertEqual(url, "https://hook?wait=true")
        self.assertEqual(body["thread_name"], "#7 Clip fades")
        self.assertTrue(body["content"].startswith("**Clip fades**\nA test build of commit `e520086`"))

    def test_posts_in_the_thread(self):
        outcome, [(url, body)] = self.run_backfill([comment()], {"id": "9", "name": "#7 x"}, [{"content": "hello"}])
        self.assertEqual(outcome, "posted")
        self.assertEqual(url, "https://hook?wait=true&thread_id=9")
        self.assertTrue(body["content"].startswith("**New build is available for testing** (commit `e520086`)."))

    def test_a_thread_with_the_commit_gets_nothing(self):
        history = [{"content": "**New build is available for testing** (commit `e520086`)."}]
        outcome, posts = self.run_backfill([comment()], {"id": "9", "name": "#7 x"}, history)
        self.assertEqual((outcome, posts), ("skipped: thread already has e520086", []))

    def test_no_comment(self):
        self.assertEqual(self.run_backfill([comment("hi")]), ("skipped: no test build comment", []))

    def test_expired_build(self):
        body = COMMENT.replace("2026-10-22", "2026-10-01")
        self.assertEqual(self.run_backfill([comment(body)]), ("skipped: build e520086 expired 2026-10-01", []))

    def test_discord_error_goes_on(self):
        error = discord_api.DiscordError("webhook: 500", 500)
        outcome, posts = self.run_backfill([comment()], error=error)
        self.assertEqual((outcome, len(posts)), ("failed", 1))

    def test_summary(self):
        text = bf.summary({7: "opened", 8: "skipped: no test build comment", 9: "opened"})
        self.assertTrue(text.startswith("opened 2, skipped 1\n- #7: opened"))


class Main(unittest.TestCase):
    def test_missing_settings_post_nothing(self):
        with mock.patch.dict("os.environ", {"PULL_REQUESTS": ""}, clear=True), mock.patch.object(bf, "github") as github:
            self.assertEqual(bf.main(), 0)
            github.assert_not_called()

    def test_malformed_numbers_fail(self):
        with mock.patch.dict("os.environ", {"PULL_REQUESTS": "7; rm"}, clear=True):
            self.assertEqual(bf.main(), 1)


if __name__ == "__main__":
    unittest.main()
