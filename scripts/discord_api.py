# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Discord's REST API for the scripts that keep the testing forum's threads (docs/releasing.md ›
Discord): calls as the bot and through a webhook, waiting out rate limits, and finding a pull
request's thread by its name, `#<number> …`. Standard library only. Neither the token nor a
webhook URL is ever printed or put in an error.
"""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://discord.com/api/v10"
USER_AGENT = "DiscordBot (https://github.com/suhina-daw/suhina-daw, 1)"
# Discord's longest thread name.
NAME_LIMIT = 100
# Discord allows two renames of a thread in ten minutes; a longer wait gives up.
MAX_WAIT = 660.0
ATTEMPTS = 4
# Archived threads are listed newest first, 100 a page.
MAX_PAGES = 20


class DiscordError(Exception):
    def __init__(self, message: str, status: int = 0):
        super().__init__(message)
        self.status = status


def send(method: str, url: str, label: str, body: dict | None = None, headers: dict | None = None) -> dict:
    """One request, retried after a 429 for as long as Discord asks (up to MAX_WAIT). `label`
    names the request in messages, in place of the URL."""
    # urllib puts a malformed URL in its error; a webhook URL is a secret, so it is checked here.
    if not url.startswith("https://"):
        raise DiscordError(f"{label}: the URL does not start with https://")
    data = json.dumps(body).encode() if body is not None else None
    for _ in range(ATTEMPTS):
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("User-Agent", USER_AGENT)
        for key, value in (headers or {}).items():
            request.add_header(key, value)
        if data is not None:
            request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                text = response.read()
                return json.loads(text) if text else {}
        except urllib.error.HTTPError as error:
            text = error.read().decode(errors="replace")
            if error.code != 429:
                raise DiscordError(f"{label}: {error.code} {text[:300]}", error.code) from None
            try:
                wait = float(json.loads(text).get("retry_after", 1.0))
            except (ValueError, AttributeError):
                wait = float(error.headers.get("Retry-After") or 1.0)
            if wait > MAX_WAIT:
                raise DiscordError(f"{label}: rate limited for {wait:.0f} s", 429) from None
            print(f"Rate limited; waiting {wait:.1f} s.")
            time.sleep(wait + 0.5)
    raise DiscordError(f"{label}: still rate limited after {ATTEMPTS} attempts", 429)


class Discord:
    """Calls as the bot."""

    def __init__(self, token: str):
        self.token = token

    def call(self, method: str, path: str, body: dict | None = None) -> dict | None:
        return send(method, API + path, f"{method} {path}", body, {"Authorization": f"Bot {self.token}"})


def prefix(number: int) -> str:
    return f"#{number} "


def is_thread_of(name: str, number: int) -> bool:
    return (name or "").startswith(prefix(number))


def cut(name: str) -> str:
    """A name cut to Discord's limit, an ellipsis showing the cut."""
    name = name.rstrip()
    return name if len(name) <= NAME_LIMIT else name[: NAME_LIMIT - 1].rstrip() + "…"


def find_thread(threads: list[dict], number: int, forum: str) -> dict | None:
    for thread in threads:
        if str(thread.get("parent_id")) == forum and is_thread_of(thread.get("name", ""), number):
            return thread
    return None


def locate(discord: Discord, guild: str, forum: str, number: int) -> dict | None:
    """The pull request's thread in the forum: the guild's active threads first, then the forum's
    archived public threads."""
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
