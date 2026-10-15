"""Tests for discord_test_build.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import json
import os
import tempfile
import unittest
from unittest import mock

import discord_api
import discord_test_build as dt


def data(**extra):
    base = {
        "number": 7,
        "title": "Clip fades:  shapes\nand a curve @everyone",
        "pr_url": "https://github.com/o/r/pull/7",
        "comment_url": "https://github.com/o/r/pull/7#issuecomment-1",
        "run_url": "https://github.com/o/r/actions/runs/5",
        "commit": "abc1234",
        "contributor": False,
        "rows": [
            {"label": "Windows x86-64", "artifact": {"name": "suhina-pr7-windows-x86_64", "url": "https://a/1", "size_mb": "45"}},
            {"label": "Linux x86-64", "state": "failure", "log": "https://l/2"},
            {"label": "macOS 11+", "state": "not built"},
        ],
    }
    base.update(extra)
    return base


class Names(unittest.TestCase):
    def test_number_first_one_line(self):
        self.assertEqual(dt.thread_name(7, "Clip fades:\n shapes"), "#7 Clip fades: shapes")

    def test_cut_to_the_limit(self):
        name = dt.thread_name(326, "word " * 40)
        self.assertEqual(len(name), discord_api.NAME_LIMIT)
        self.assertTrue(name.startswith("#326 word"))
        self.assertTrue(name.endswith("…"))

    def test_empty_title(self):
        self.assertEqual(dt.thread_name(7, "  "), "#7")


class Messages(unittest.TestCase):
    def test_first_post(self):
        text = dt.message(data(), True)
        self.assertEqual(
            text,
            "**Clip fades: shapes and a curve @everyone**\n"
            "A test build of commit `abc1234` is ready to try.\n"
            "\n"
            "- Windows x86-64: [suhina-pr7-windows-x86_64](<https://a/1>) (45 MB)\n"
            "- Linux x86-64: failure ([log](<https://l/2>))\n"
            "- macOS 11+: not built\n"
            "\n"
            "Downloads and how to run them: <https://github.com/o/r/pull/7#issuecomment-1>\n"
            "Pull request: <https://github.com/o/r/pull/7>\n"
            "Downloading needs a GitHub sign-in.",
        )

    def test_update(self):
        text = dt.message(data(contributor=True, comment_url=None), False)
        self.assertTrue(text.startswith("**New build is available for testing** (commit `abc1234`)."))
        self.assertIn("**A contributor's build.**", text)
        self.assertIn("<https://github.com/o/r/actions/runs/5>", text)

    def test_long_message_is_cut(self):
        rows = [{"label": "x" * 100, "state": "failure"}] * 40
        self.assertEqual(len(dt.message(data(rows=rows), True)), dt.LIMIT)

    def test_payload_pings_no_one(self):
        self.assertEqual(dt.payload("hi"), {"content": "hi", "allowed_mentions": {"parse": []}})
        self.assertEqual(dt.payload("hi", "#7 x")["thread_name"], "#7 x")


class Posting(unittest.TestCase):
    def run_post(self, thread, build=None, error=None):
        calls = []

        def send(method, url, label, body=None, headers=None):
            calls.append((url, body))
            if error:
                raise error
            return {"channel_id": "77"}

        with mock.patch.object(dt, "locate", return_value=thread), mock.patch.object(dt, "send", send):
            dt.post("https://hook", None, "1", "500", build or data())
        return calls

    def test_new_thread(self):
        [(url, body)] = self.run_post(None)
        self.assertEqual(url, "https://hook?wait=true")
        self.assertEqual(body["thread_name"], "#7 Clip fades: shapes and a curve @everyone")

    def test_existing_thread_is_not_renamed(self):
        [(url, body)] = self.run_post({"id": "9", "name": "#7 Old title"})
        self.assertEqual(url, "https://hook?wait=true&thread_id=9")
        self.assertNotIn("thread_name", body)
        self.assertTrue(body["content"].startswith("**New build"))

    def test_no_thread_and_nothing_built(self):
        self.assertEqual(self.run_post(None, data(rows=[{"label": "x", "state": "failure"}])), [])

    def test_refused_post_is_logged(self):
        error = discord_api.DiscordError("webhook: 400", 400)
        self.assertEqual(len(self.run_post({"id": "9", "name": "#7 x"}, error=error)), 1)


class Urls(unittest.TestCase):
    def test_a_malformed_webhook_is_never_in_the_error(self):
        with mock.patch("urllib.request.urlopen") as urlopen:
            with self.assertRaises(discord_api.DiscordError) as caught:
                discord_api.send("POST", "discord.com/api/webhooks/1/secret-token?wait=true", "webhook", {})
            urlopen.assert_not_called()
        self.assertEqual(str(caught.exception), "webhook: the URL does not start with https://")

    def test_main_logs_it_and_exits_0(self):
        env = {
            "DISCORD_WEBHOOK_TESTING": "secret-token",
            "DISCORD_BOT_TOKEN": "t",
            "DISCORD_GUILD_ID": "1",
            "DISCORD_TESTING_FORUM_ID": "500",
        }
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "build.json")
            with open(path, "w", encoding="utf-8") as f:
                json.dump(data(), f)
            with mock.patch.dict("os.environ", env, clear=True), mock.patch.object(dt, "locate", return_value=None), \
                    mock.patch("builtins.print") as printed:
                self.assertEqual(dt.main([path]), 0)
        text = " ".join(str(call) for call in printed.call_args_list)
        self.assertIn("does not start with https://", text)
        self.assertNotIn("secret-token", text)


if __name__ == "__main__":
    unittest.main()
