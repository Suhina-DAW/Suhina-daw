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
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://discord.com/api/v10"
MARK = "[Merged]"
REACTION = "✅"
# Discord's longest thread name.
NAME_LIMIT = 100
# Discord allows two renames of a thread in ten minutes; a longer wait gives up.
MAX_WAIT = 660.0
ATTEMPTS = 4
# Archived threads are listed newest first, 100 a page.
MAX_PAGES = 20


def prefix(number: int) -> str:
    return f"#{number} "


def is_thread_of(name: str, number: int) -> bool:
    return (name or "").startswith(prefix(number))


def is_marked(name: str, number: int) -> bool:
    return (name or "").startswith(prefix(number) + MARK)


def merged_name(name: str, number: int) -> str | None:
    """`#280 Title` as `#280 [Merged] Title`, cut to Discord's limit; None when the name is not
    the pull request's thread or is already marked."""
    if not is_thread_of(name, number) or is_marked(name, number):
        return None
    head = f"{prefix(number)}{MARK} "
    rest = name[len(prefix(number)) :].strip()
    text = (head + rest).rstrip()
    if len(text) > NAME_LIMIT:
        text = text[: NAME_LIMIT - 1].rstrip() + "…"
    return text


def find_thread(threads: list[dict], number: int, forum: str) -> dict | None:
    for thread in threads:
        if str(thread.get("parent_id")) == forum and is_thread_of(thread.get("name", ""), number):
            return thread
    return None


def merged_number(event: dict) -> int | None:
    pr = event.get("pull_request") or {}
    if event.get("action") != "closed" or not pr.get("merged"):
        return None
    number = pr.get("number")
    return number if isinstance(number, int) else None


class Discord:
    def __init__(self, token: str):
        self.token = token

    def call(self, method: str, path: str, body: dict | None = None) -> dict | None:
        data = json.dumps(body).encode() if body is not None else None
        for _ in range(ATTEMPTS):
            request = urllib.request.Request(API + path, data=data, method=method)
            request.add_header("Authorization", f"Bot {self.token}")
            request.add_header("User-Agent", "DiscordBot (https://github.com/suhina-daw/suhina-daw, 1)")
            if data is not None:
                request.add_header("Content-Type", "application/json")
            try:
                with urllib.request.urlopen(request, timeout=30) as response:
                    text = response.read()
                    return json.loads(text) if text else {}
            except urllib.error.HTTPError as error:
                text = error.read().decode(errors="replace")
                if error.code != 429:
                    raise DiscordError(f"{method} {path}: {error.code} {text[:300]}") from None
                try:
                    wait = float(json.loads(text).get("retry_after", 1.0))
                except (ValueError, AttributeError):
                    wait = float(error.headers.get("Retry-After") or 1.0)
                if wait > MAX_WAIT:
                    raise DiscordError(f"{method} {path}: rate limited for {wait:.0f} s") from None
                print(f"Rate limited; waiting {wait:.1f} s.")
                time.sleep(wait + 0.5)
        raise DiscordError(f"{method} {path}: still rate limited after {ATTEMPTS} attempts")


class DiscordError(Exception):
    pass


def locate(discord: Discord, guild: str, forum: str, number: int) -> dict | None:
    active = discord.call("GET", f"/guilds/{guild}/threads/active") or {}
    found = find_thread(active.get("threads") or [], number, forum)
    if found:
        return found
    before = ""
    for _ in range(MAX_PAGES):
        query = "?limit=100" + (f"&before={urllib.parse.quote(before)}" if before else "")
        page = discord.call("GET", f"/channels/{forum}/threads/archived/public{query}") or {}
        threads = page.get("threads") or []
        found = find_thread(threads, number, forum)
        if found or not threads or not page.get("has_more"):
            return found
        before = (threads[-1].get("thread_metadata") or {}).get("archive_timestamp") or ""
        if not before:
            return None
    return None


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
