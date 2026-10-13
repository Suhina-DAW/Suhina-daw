#!/usr/bin/env python3
"""The Discord post for a GitHub event, for .github/workflows/discord.yml (docs/releasing.md).

    discord_post.py EVENT_NAME EVENT_JSON_FILE

Prints `<channel> <JSON payload>` on one line, the channel being `events` or `releases`, or
nothing when the event is not posted. Pull requests (opened, merged, closed unmerged, reopened)
and issues (opened, closed, reopened) go to the events channel; a published release goes to the
releases channel with its "New and changed" section. The nightly is posted by release.yml, not
here. Standard library only; the event's text is data in the JSON payload, never a command.
"""

from __future__ import annotations

import json
import re
import sys

# Discord rejects longer messages.
LIMIT = 2000


def payload(content: str) -> str:
    # allowed_mentions: a title or a body never pings anyone.
    return json.dumps({"content": content[:LIMIT], "allowed_mentions": {"parse": []}})


def one_line(text: str, length: int = 200) -> str:
    text = " ".join((text or "").split())
    return text if len(text) <= length else text[: length - 1] + "…"


def pull_request(event: dict) -> tuple[str, str] | None:
    pr = event.get("pull_request") or {}
    action = event.get("action")
    if action == "closed":
        action = "merged" if pr.get("merged") else "closed"
    if action not in ("opened", "merged", "closed", "reopened"):
        return None
    user = (pr.get("user") or {}).get("login", "someone")
    draft = " (draft)" if pr.get("draft") and action == "opened" else ""
    text = (
        f"**Pull request #{pr.get('number')} {action}**{draft}: {one_line(pr.get('title'))}"
        f" (by {user}) <{pr.get('html_url')}>"
    )
    return "events", text


def issue(event: dict) -> tuple[str, str] | None:
    item = event.get("issue") or {}
    action = event.get("action")
    if action not in ("opened", "closed", "reopened"):
        return None
    if action == "closed" and item.get("state_reason") == "not_planned":
        action = "closed (not planned)"
    user = (item.get("user") or {}).get("login", "someone")
    text = (
        f"**Issue #{item.get('number')} {action}**: {one_line(item.get('title'))}"
        f" (by {user}) <{item.get('html_url')}>"
    )
    return "events", text


def new_and_changed(body: str) -> str:
    """The "New and changed" section of release notes (scripts/release_notes.py), without its
    heading, HTML comments left out."""
    body = re.sub(r"<!--.*?-->", "", body or "", flags=re.S)
    m = re.search(r"^## New and changed\s*\n(.*?)(?=^## |\Z)", body, flags=re.S | re.M)
    return m.group(1).strip() if m else ""


def release(event: dict) -> tuple[str, str] | None:
    rel = event.get("release") or {}
    if event.get("action") != "published" or rel.get("draft") or rel.get("tag_name") == "nightly":
        return None
    url = rel.get("html_url")
    head = f"**{rel.get('name') or rel.get('tag_name')}** is out: <{url}>"
    section = new_and_changed(rel.get("body") or "")
    if not section:
        return "releases", head
    more = f"\n…and more on the release page: <{url}>"
    text = f"{head}\n\n{section}"
    if len(text) > LIMIT:
        # Whole lines only, then the link to the rest.
        lines = text.splitlines()
        while lines and len("\n".join(lines)) + len(more) > LIMIT:
            lines.pop()
        text = "\n".join(lines) + more
    return "releases", text


HANDLERS = {
    "pull_request_target": pull_request,
    "pull_request": pull_request,
    "issues": issue,
    "release": release,
}


def post_for(name: str, event: dict) -> tuple[str, str] | None:
    handler = HANDLERS.get(name)
    return handler(event) if handler else None


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as f:
        event = json.load(f)
    post = post_for(argv[0], event)
    if post:
        channel, content = post
        print(f"{channel} {payload(content)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
