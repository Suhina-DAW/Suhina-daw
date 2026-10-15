#!/usr/bin/env python3
"""Marks a merged pull request's thread in the Discord testing forum, for
.github/workflows/discord.yml (docs/releasing.md › Discord).

    discord_merged.py EVENT_JSON_FILE

For a merged pull request, the bot finds the forum thread whose name starts with `#<number> `
(the guild's active threads first, then the forum's archived public threads), adds ✅ to its
starter message, renames it to `#<number> [Merged] <title>` and archives it, the rename and the
archiving in one request. A thread already marked is not renamed again.

Environment: DISCORD_BOT_TOKEN, DISCORD_GUILD_ID and DISCORD_TESTING_FORUM_ID. Without them, or
with no thread for the pull request, it logs and does nothing. It always exits 0: a Discord
error is a warning, never a failed run. Standard library only.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.parse

from discord_api import Discord, DiscordError, cut, is_thread_of, locate, prefix

MARK = "[Merged]"
REACTION = "✅"


def is_marked(name: str, number: int) -> bool:
    return (name or "").startswith(prefix(number) + MARK)


def merged_name(name: str, number: int) -> str | None:
    """`#280 Title` as `#280 [Merged] Title`, cut to Discord's limit; None when the name is not
    the pull request's thread or is already marked."""
    if not is_thread_of(name, number) or is_marked(name, number):
        return None
    rest = name[len(prefix(number)) :].strip()
    return cut(f"{prefix(number)}{MARK} {rest}")


def merged_number(event: dict) -> int | None:
    pr = event.get("pull_request") or {}
    if event.get("action") != "closed" or not pr.get("merged"):
        return None
    number = pr.get("number")
    return number if isinstance(number, int) else None


def mark(discord: Discord, thread: dict, number: int) -> None:
    tid = thread["id"]
    name = thread.get("name", "")
    archived = (thread.get("thread_metadata") or {}).get("archived", False)
    new_name = merged_name(name, number)
    if archived and new_name is None:
        print(f"Thread {tid} ({name}) is already marked and archived.")
        return
    if archived:
        # Discord takes no reaction on an archived thread's messages.
        discord.call("PATCH", f"/channels/{tid}", {"archived": False})
    emoji = urllib.parse.quote(REACTION)
    try:
        # A forum post's starter message has the thread's id.
        discord.call("PUT", f"/channels/{tid}/messages/{tid}/reactions/{emoji}/@me")
    except DiscordError as error:
        print(f"::warning::No reaction on thread {tid}: {error}")
    change: dict = {"archived": True}
    if new_name is not None:
        change["name"] = new_name
    discord.call("PATCH", f"/channels/{tid}", change)
    print(f"Thread {tid}: {name!r} -> {change.get('name', name)!r}, archived.")


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[0], encoding="utf-8") as f:
        number = merged_number(json.load(f))
    if number is None:
        print("Not a merged pull request; nothing to do.")
        return 0
    token = os.environ.get("DISCORD_BOT_TOKEN", "")
    guild = os.environ.get("DISCORD_GUILD_ID", "")
    forum = os.environ.get("DISCORD_TESTING_FORUM_ID", "")
    if not (token and guild and forum):
        print("DISCORD_BOT_TOKEN, DISCORD_GUILD_ID or DISCORD_TESTING_FORUM_ID missing; nothing done.")
        return 0
    discord = Discord(token)
    try:
        thread = locate(discord, guild, forum, number)
        if thread is None:
            print(f"No thread for #{number} in the testing forum.")
            return 0
        mark(discord, thread, number)
    except (DiscordError, OSError, ValueError, KeyError) as error:
        print(f"::warning::Marking the thread of #{number} failed: {error}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
