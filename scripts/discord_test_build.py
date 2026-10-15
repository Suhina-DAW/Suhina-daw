#!/usr/bin/env python3
"""Posts a linked test build to its pull request's thread in the Discord testing forum, for
.github/workflows/test-build-links.yml (docs/releasing.md › Discord).

    discord_test_build.py BUILD_JSON_FILE

BUILD_JSON_FILE is written by test-build-links.yml from the same data as the pull request
comment: {number, title, pr_url, comment_url, run_url, commit, contributor, rows}, each row
{label, artifact: {name, url, size_mb}} for a platform that built, or {label, state, log}.

The bot finds the thread whose name starts with `#<number> ` (scripts/discord_api.py); it never
posts. The webhook posts: into the thread when there is one (which unarchives it), or as a new
thread named `#<number> <title>` when there is none and at least one platform built. An existing
thread is never renamed.

Environment: DISCORD_WEBHOOK_TESTING, DISCORD_BOT_TOKEN, DISCORD_GUILD_ID and
DISCORD_TESTING_FORUM_ID. Without them it logs and does nothing. It always exits 0: a Discord
error is a warning, never a failed run. Standard library only; the title is data in the JSON
payload, and the post pings no one.
"""

from __future__ import annotations

import json
import os
import sys

from discord_api import Discord, DiscordError, cut, locate, prefix, send

# Discord rejects longer messages.
LIMIT = 2000


def one_line(text: str) -> str:
    return " ".join((text or "").split())


def thread_name(number: int, title: str) -> str:
    """`#<number> <title>`, the number first, cut to Discord's limit."""
    return cut(f"{prefix(number)}{one_line(title)}") or f"#{number}"


def built(data: dict) -> bool:
    return any(row.get("artifact") for row in data.get("rows") or [])


def row_line(row: dict) -> str:
    label = row.get("label", "?")
    artifact = row.get("artifact")
    if artifact:
        return f"- {label}: [{artifact.get('name')}](<{artifact.get('url')}>) ({artifact.get('size_mb')} MB)"
    state = row.get("state") or "not built"
    log = row.get("log")
    return f"- {label}: {state}" + (f" ([log](<{log}>))" if log else "")


def message(data: dict, first: bool) -> str:
    commit = data.get("commit", "")
    if first:
        head = [f"**{one_line(data.get('title'))}**", f"A test build of commit `{commit}` is ready to try."]
    else:
        head = [f"**New build is available for testing** (commit `{commit}`)."]
    lines = head + [""] + [row_line(row) for row in data.get("rows") or []] + [""]
    if data.get("contributor"):
        lines.append("**A contributor's build.** Read the diff before you run it.")
    lines += [
        f"Downloads and how to run them: <{data.get('comment_url') or data.get('run_url')}>",
        f"Pull request: <{data.get('pr_url')}>",
        "Downloading needs a GitHub sign-in.",
    ]
    text = "\n".join(lines)
    return text if len(text) <= LIMIT else text[: LIMIT - 1] + "…"


def payload(content: str, name: str | None = None) -> dict:
    # allowed_mentions: a title never pings anyone.
    body: dict = {"content": content, "allowed_mentions": {"parse": []}}
    if name is not None:
        body["thread_name"] = name
    return body


def post(webhook: str, discord: Discord, guild: str, forum: str, data: dict) -> str:
    return post_to(webhook, locate(discord, guild, forum, data["number"]), data)


def post_to(webhook: str, thread: dict | None, data: dict) -> str:
    """Posts into `thread`, or opens the pull request's thread when it is None. Returns what was
    done: `posted`, `opened`, `refused` or `nothing`."""
    number = data["number"]
    if thread is not None:
        tid = thread["id"]
        try:
            send("POST", f"{webhook}?wait=true&thread_id={tid}", "webhook", payload(message(data, False)))
        except DiscordError as error:
            # A locked thread refuses the webhook; it is not unarchived either.
            print(f"::warning::Thread {tid} ({thread.get('name')}) refused the post: {error}")
            return "refused"
        print(f"Posted the build of #{number} in thread {tid} ({thread.get('name')}).")
        return "posted"
    if not built(data):
        print(f"No thread for #{number} and no platform built; nothing posted.")
        return "nothing"
    name = thread_name(number, data.get("title", ""))
    reply = send("POST", f"{webhook}?wait=true", "webhook", payload(message(data, True), name))
    # In a forum the new message's channel is the new thread.
    print(f"Opened thread {reply.get('channel_id')} ({name}).")
    return "opened"


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[0], encoding="utf-8") as f:
        data = json.load(f)
    webhook = os.environ.get("DISCORD_WEBHOOK_TESTING", "")
    token = os.environ.get("DISCORD_BOT_TOKEN", "")
    guild = os.environ.get("DISCORD_GUILD_ID", "")
    forum = os.environ.get("DISCORD_TESTING_FORUM_ID", "")
    if not (webhook and token and guild and forum):
        print("A Discord secret or variable is missing; nothing posted.")
        return 0
    if not isinstance(data.get("number"), int):
        print("No pull request number in the build data; nothing posted.")
        return 0
    try:
        post(webhook, Discord(token), guild, forum, data)
    except (DiscordError, OSError, ValueError, KeyError) as error:
        print(f"::warning::Posting the build of #{data.get('number')} failed: {error}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
