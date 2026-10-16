#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Posts the test builds already linked from open pull requests to the Discord testing forum, for
the manual run of .github/workflows/test-build-links.yml (docs/releasing.md › Discord).

    discord_test_build_backfill.py

For each open pull request, or each number in PULL_REQUESTS (`302 303`, commas and `#` allowed),
it reads the latest test build comment the link job wrote (`<!-- suhina-test-build -->`, by a
bot), turns it into the data test-build-links.yml writes for scripts/discord_test_build.py, and
posts it with that script: the thread is opened, or the build is posted in it, as for a new
build. A build whose artifacts have expired is skipped, and so is a thread that already has a
post of the same commit, so a second run posts nothing twice.

Environment: GH_TOKEN and GITHUB_REPOSITORY for reading the pull requests, PULL_REQUESTS, and the
Discord settings of discord_test_build.py. Without a Discord setting it logs and does nothing.
A Discord error is a warning and the run goes on; only a malformed PULL_REQUESTS fails it.
Standard library only.
"""

from __future__ import annotations

import datetime
import json
import os
import re
import sys
import urllib.request

from discord_api import Discord, DiscordError, locate
from discord_test_build import post_to

MARKER = "<!-- suhina-test-build -->"
GITHUB = "https://api.github.com"
# A thread's latest messages that are searched for an earlier post of the same commit.
HISTORY = 100

COMMIT = re.compile(r"^Commit `([0-9a-f]{7,40})` merged with main(?:, kept until (\d{4}-\d{2}-\d{2}))?\. \[The run\]\((\S+)\)\.$")
ARTIFACT = re.compile(r"^\[([^\]]+)\]\((\S+)\) \((\d+) MB\)$")
STATE = re.compile(r"^(.*?)(?: \(\[log\]\((\S+)\)\))?$")


def numbers(text: str) -> list[int] | None:
    """The pull request numbers in `text`, an empty list for none, None when one is malformed."""
    found = []
    for word in (text or "").replace(",", " ").replace("#", " ").split():
        if not re.fullmatch(r"[1-9][0-9]{0,6}", word):
            return None
        found.append(int(word))
    return found


def parse_comment(body: str) -> dict | None:
    """The build a link comment describes: {commit, run_url, expires, contributor, rows}, the
    rows as test-build-links.yml writes them. None when the body is not one."""
    if not (body or "").startswith(MARKER):
        return None
    build: dict = {"rows": [], "contributor": False, "expires": None}
    table = False
    for line in body.splitlines():
        line = line.strip()
        match = COMMIT.match(line)
        if match:
            build["commit"], build["expires"], build["run_url"] = match.groups()
        elif line.startswith("| Platform |"):
            table = True
        elif table and line.startswith("|---"):
            continue
        elif table and line.startswith("|"):
            cells = [cell.strip() for cell in line.strip("|").split("|")]
            if len(cells) < 2:
                continue
            label, download = cells[0], cells[1]
            artifact = ARTIFACT.match(download)
            if artifact:
                name, url, size = artifact.groups()
                build["rows"].append({"label": label, "artifact": {"name": name, "url": url, "size_mb": size}})
            else:
                state, log = STATE.match(download).groups()
                build["rows"].append({"label": label, "state": state or "not built", "log": log})
        else:
            table = False
            if line.startswith("> **A contributor's build.**"):
                build["contributor"] = True
    if "commit" not in build or not build["rows"]:
        return None
    return build


def build_data(pr: dict, comment: dict, build: dict) -> dict:
    """The JSON discord_test_build.py reads, as the link job writes it."""
    return {
        "number": pr["number"],
        "title": pr.get("title", ""),
        "pr_url": pr.get("html_url"),
        "comment_url": comment.get("html_url"),
        "run_url": build.get("run_url"),
        "commit": build["commit"],
        "contributor": build["contributor"],
        "rows": build["rows"],
    }


def link_comment(comments: list[dict]) -> dict | None:
    """The link job's comment, the latest if there are several."""
    found = None
    for comment in comments:
        if (comment.get("user") or {}).get("type") == "Bot" and (comment.get("body") or "").startswith(MARKER):
            found = comment
    return found


def expired(build: dict, today: datetime.date) -> bool:
    try:
        return build.get("expires") is not None and datetime.date.fromisoformat(build["expires"]) < today
    except ValueError:
        return False


def posted_before(discord: Discord, thread: dict, commit: str) -> bool:
    """Whether the thread's latest messages already hold a post of this commit."""
    messages = discord.call("GET", f"/channels/{thread['id']}/messages?limit={HISTORY}") or []
    return any(f"commit `{commit}`" in (m.get("content") or "") for m in messages)


def github(token: str, path: str) -> list:
    """Every page of a GitHub list."""
    items: list = []
    url: str | None = f"{GITHUB}{path}{'&' if '?' in path else '?'}per_page=100"
    while url:
        request = urllib.request.Request(url)
        request.add_header("Authorization", f"Bearer {token}")
        request.add_header("Accept", "application/vnd.github+json")
        with urllib.request.urlopen(request, timeout=30) as response:
            items += json.loads(response.read())
            links = response.headers.get("Link") or ""
        url = next((part.split(";")[0].strip(" <>") for part in links.split(",") if 'rel="next"' in part), None)
    return items


def backfill(prs: list[dict], comments_of, discord: Discord, webhook: str, guild: str, forum: str, today: datetime.date) -> dict:
    """Posts each pull request's build; returns {number: outcome}."""
    outcomes: dict = {}
    for pr in prs:
        number = pr["number"]
        comment = link_comment(comments_of(number))
        build = parse_comment(comment["body"]) if comment else None
        if build is None:
            outcomes[number] = "skipped: no test build comment"
        elif expired(build, today):
            outcomes[number] = f"skipped: build {build['commit']} expired {build['expires']}"
        else:
            data = build_data(pr, comment, build)
            try:
                thread = locate(discord, guild, forum, number)
                if thread is not None and posted_before(discord, thread, build["commit"]):
                    outcomes[number] = f"skipped: thread already has {build['commit']}"
                else:
                    outcomes[number] = post_to(webhook, thread, data)
            except (DiscordError, OSError, ValueError, KeyError) as error:
                print(f"::warning::Posting the build of #{number} failed: {error}")
                outcomes[number] = "failed"
        print(f"#{number}: {outcomes[number]}")
    return outcomes


def summary(outcomes: dict) -> str:
    counts: dict = {}
    for outcome in outcomes.values():
        key = outcome.split(":")[0]
        counts[key] = counts.get(key, 0) + 1
    lines = [", ".join(f"{key} {count}" for key, count in sorted(counts.items())) or "no pull requests"]
    lines += [f"- #{number}: {outcome}" for number, outcome in outcomes.items()]
    return "\n".join(lines)


def main() -> int:
    wanted = numbers(os.environ.get("PULL_REQUESTS", ""))
    if wanted is None:
        print("::error::PULL_REQUESTS holds something that is not a pull request number")
        return 1
    webhook = os.environ.get("DISCORD_WEBHOOK_TESTING", "")
    token = os.environ.get("DISCORD_BOT_TOKEN", "")
    guild = os.environ.get("DISCORD_GUILD_ID", "")
    forum = os.environ.get("DISCORD_TESTING_FORUM_ID", "")
    if not (webhook and token and guild and forum):
        print("A Discord secret or variable is missing; nothing posted.")
        return 0
    gh = os.environ.get("GH_TOKEN", "")
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    prs = github(gh, f"/repos/{repo}/pulls?state=open")
    if wanted:
        prs = [pr for pr in prs if pr["number"] in wanted]
        for number in sorted(set(wanted) - {pr["number"] for pr in prs}):
            print(f"#{number} is not an open pull request; skipped.")
    prs.sort(key=lambda pr: pr["number"])
    outcomes = backfill(
        prs,
        lambda number: github(gh, f"/repos/{repo}/issues/{number}/comments"),
        Discord(token),
        webhook,
        guild,
        forum,
        datetime.datetime.now(datetime.timezone.utc).date(),
    )
    text = summary(outcomes)
    print(text)
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write("### Test builds posted to Discord\n\n" + text + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
