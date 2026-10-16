#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Brings the Suhina project board up to date with the repository (docs/claim-bot.md › Project sync).

    project_sync.py [--dry-run | --apply]

It reads the repository's open issues and open pull requests and every card on the board over the
GraphQL API, then:

- adds every open issue and open pull request of the repository that has no card, in `Backlog`
  unless a rule below gives it another Status;
- sets each card's Status from the repository's state:
  - an open issue with an open pull request that closes it or says `Part of #N`: `In review`;
  - an open issue that is claimed (a `/claim` comment not followed by `/unclaim`, or an assignee)
    and has no such pull request: `In progress`, except on a held card (`Do not start yet`), which
    only a pull request or closing the issue moves;
  - a closed issue or a merged pull request: `Done`;
  - every other card keeps its Status.

It never touches Priority, Phase or any field other than Status, and never deletes a card. The
default is a dry run that prints every add and move and writes nothing; `--apply` writes them.

Environment: PROJECT_TOKEN (the claim bot's token), PROJECT_OWNER (default `suhina-daw`),
PROJECT_NUMBER (default 1) and GITHUB_REPOSITORY (default `suhina-daw/suhina-daw`). When
GITHUB_STEP_SUMMARY is set, the same report is written there too. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass, field

API = "https://api.github.com/graphql"

# The board's words, as the claim bot (.github/workflows/claim.yml) uses them. Matched without case.
BACKLOG = "Backlog"
IN_PROGRESS = "In progress"
IN_REVIEW = "In review"
DONE = "Done"
HELD = "Do not start yet"

# The claim bot's answers that refuse a command: a `/claim` or `/unclaim` it answered with one of
# these did not take effect. Every other answer (Claimed by, Command noted, an error) leaves it in
# force, because the bot says a comment is still a claim when it cannot do its job.
REFUSALS = (
    "This is ",
    "There is already a pull request",
    "An `epic`",
    "This feature is not agreed",
    "Only the person who claimed",
    "Only a maintainer",
)
# A refusal that starts with the commenter's name rather than a fixed phrase.
REFUSED_ANYWHERE = ("nothing was claimed",)

PAGE = 50
# Comments read per issue, newest last: enough to find the latest `/claim` or `/unclaim`.
COMMENTS = 100
# A guard against a cursor that never ends.
MAX_PAGES = 100

BOARD_QUERY = """
query ($owner: String!, $number: Int!, $after: String) {
  organization(login: $owner) {
    projectV2(number: $number) {
      id
      field(name: "Status") {
        ... on ProjectV2SingleSelectField { id options { id name } }
      }
      items(first: 100, after: $after) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id
          fieldValueByName(name: "Status") {
            ... on ProjectV2ItemFieldSingleSelectValue { name }
          }
          content {
            __typename
            ... on Issue { id number title state repository { nameWithOwner } }
            ... on PullRequest { id number title state merged repository { nameWithOwner } }
          }
        }
      }
    }
  }
}
"""

ISSUES_QUERY = """
query ($owner: String!, $repo: String!, $after: String) {
  repository(owner: $owner, name: $repo) {
    issues(first: %d, after: $after, states: OPEN) {
      pageInfo { hasNextPage endCursor }
      nodes {
        id
        number
        title
        assignees(first: 10) { nodes { login } }
        comments(last: %d) { nodes { body author { __typename login } } }
      }
    }
  }
}
""" % (PAGE, COMMENTS)

PULLS_QUERY = """
query ($owner: String!, $repo: String!, $after: String) {
  repository(owner: $owner, name: $repo) {
    pullRequests(first: %d, after: $after, states: OPEN) {
      pageInfo { hasNextPage endCursor }
      nodes {
        id
        number
        title
        body
        closingIssuesReferences(first: 20) { nodes { number repository { nameWithOwner } } }
      }
    }
  }
}
""" % PAGE

ADD_MUTATION = """
mutation ($project: ID!, $content: ID!) {
  addProjectV2ItemById(input: { projectId: $project, contentId: $content }) { item { id } }
}
"""

SET_MUTATION = """
mutation ($project: ID!, $item: ID!, $field: ID!, $option: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $project, itemId: $item, fieldId: $field,
    value: { singleSelectOptionId: $option }
  }) { projectV2Item { id } }
}
"""

PART_OF = re.compile(r"\bpart\s+of\s+((?:#\d+(?:\s*(?:,|and|&)\s*)?)+)", re.IGNORECASE)
CLOSES = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+#(\d+)\b", re.IGNORECASE
)
NUMBER = re.compile(r"#(\d+)")


class SyncError(Exception):
    pass


@dataclass
class Card:
    item_id: str
    status: str | None
    kind: str  # "Issue" or "PullRequest"
    number: int
    title: str
    state: str  # OPEN, CLOSED or MERGED
    merged: bool
    repo: str


@dataclass
class Board:
    project_id: str
    field_id: str
    options: dict[str, tuple[str, str]]  # lower-case name -> (option id, name as on the board)
    cards: list[Card] = field(default_factory=list)

    def option(self, name: str) -> str:
        found = self.options.get(name.lower())
        if found is None:
            names = ", ".join(name for _, name in self.options.values())
            raise SyncError(f"the board's Status field has no option {name!r}; it has: {names}")
        return found[0]


@dataclass
class Issue:
    node_id: str
    number: int
    title: str
    assignees: list[str]
    comments: list[dict]


@dataclass
class Pull:
    node_id: str
    number: int
    title: str
    closes: set[int]
    part_of: set[int]


@dataclass
class Action:
    kind: str  # "add" or "move"
    content: str  # "issue" or "pull request"
    number: int
    title: str
    status: str
    before: str | None = None
    item_id: str | None = None
    node_id: str | None = None
    reason: str = ""

    def describe(self) -> str:
        what = f"{self.content} #{self.number} {self.title}"
        why = f" ({self.reason})" if self.reason else ""
        if self.kind == "add":
            return f"add   {what} -> {self.status}{why}"
        return f"move  {what}: {self.before or 'no status'} -> {self.status}{why}"


def same(a: str | None, b: str) -> bool:
    return isinstance(a, str) and a.lower() == b.lower()


# ---- Reading ---------------------------------------------------------------------------------


def paged(graphql, query: str, variables: dict, path: list[str]) -> list[dict]:
    """Every node of a connection at `path`, page by page."""
    nodes: list[dict] = []
    after = None
    for _ in range(MAX_PAGES):
        data = graphql(query, {**variables, "after": after})
        connection = data
        for key in path:
            connection = (connection or {}).get(key)
        if connection is None:
            raise SyncError(f"no {'.'.join(path)} in the answer; is the token allowed to read it?")
        nodes.extend(node for node in connection.get("nodes") or [] if node)
        info = connection.get("pageInfo") or {}
        if not info.get("hasNextPage"):
            return nodes
        after = info.get("endCursor")
    raise SyncError(f"more than {MAX_PAGES} pages of {'.'.join(path)}")


def read_board(graphql, owner: str, number: int) -> Board:
    first = graphql(BOARD_QUERY, {"owner": owner, "number": number, "after": None})
    project = ((first.get("organization") or {}).get("projectV2")) or None
    status = (project or {}).get("field")
    if not project or not status:
        raise SyncError(f"no project {owner}/{number} with a Status field is visible to PROJECT_TOKEN")
    options = {
        option["name"].lower(): (option["id"], option["name"])
        for option in status.get("options") or []
    }
    board = Board(project["id"], status["id"], options)
    items = paged(
        graphql,
        BOARD_QUERY,
        {"owner": owner, "number": number},
        ["organization", "projectV2", "items"],
    )
    for node in items:
        content = node.get("content") or {}
        kind = content.get("__typename")
        if kind not in ("Issue", "PullRequest"):
            continue  # a draft card, or one this token cannot see
        value = node.get("fieldValueByName") or {}
        merged = bool(content.get("merged"))
        board.cards.append(
            Card(
                item_id=node["id"],
                status=value.get("name"),
                kind=kind,
                number=content.get("number", 0),
                title=content.get("title", ""),
                state="MERGED" if merged else content.get("state", ""),
                merged=merged,
                repo=(content.get("repository") or {}).get("nameWithOwner", ""),
            )
        )
    return board


def read_issues(graphql, owner: str, repo: str) -> list[Issue]:
    nodes = paged(graphql, ISSUES_QUERY, {"owner": owner, "repo": repo}, ["repository", "issues"])
    return [
        Issue(
            node_id=node["id"],
            number=node["number"],
            title=node.get("title", ""),
            assignees=[a["login"] for a in (node.get("assignees") or {}).get("nodes") or [] if a],
            comments=[c for c in (node.get("comments") or {}).get("nodes") or [] if c],
        )
        for node in nodes
    ]


def references(text: str) -> tuple[set[int], set[int]]:
    """Issue numbers a pull request's text closes and says it is part of."""
    closes = {int(n) for n in CLOSES.findall(text or "")}
    part_of: set[int] = set()
    for group in PART_OF.findall(text or ""):
        part_of.update(int(n) for n in NUMBER.findall(group))
    return closes, part_of


def read_pulls(graphql, owner: str, repo: str) -> list[Pull]:
    slug = f"{owner}/{repo}"
    nodes = paged(
        graphql, PULLS_QUERY, {"owner": owner, "repo": repo}, ["repository", "pullRequests"]
    )
    pulls = []
    for node in nodes:
        text = f"{node.get('title') or ''}\n{node.get('body') or ''}"
        closes, part_of = references(text)
        for ref in (node.get("closingIssuesReferences") or {}).get("nodes") or []:
            if ref and (ref.get("repository") or {}).get("nameWithOwner", slug) == slug:
                closes.add(ref["number"])
        pulls.append(Pull(node["id"], node["number"], node.get("title", ""), closes, part_of))
    return pulls


# ---- Deciding --------------------------------------------------------------------------------


def command(body: str) -> str | None:
    head = (body or "").strip().split("\n", 1)[0].strip()
    match = re.match(r"^/(claim|unclaim)\b", head, re.IGNORECASE)
    return match.group(1).lower() if match else None


def is_bot(comment: dict) -> bool:
    author = comment.get("author") or {}
    return author.get("__typename") == "Bot" or author.get("login", "").endswith("[bot]")


def claimed_in_comments(comments: list[dict]) -> bool:
    """Whether the newest `/claim` or `/unclaim` that the claim bot did not refuse is a `/claim`."""
    state = False
    pending: str | None = None
    for comment in comments:
        if is_bot(comment):
            if pending is not None:
                answer = comment.get("body") or ""
                if not (answer.startswith(REFUSALS) or any(r in answer for r in REFUSED_ANYWHERE)):
                    state = pending == "claim"
                pending = None
            continue
        found = command(comment.get("body") or "")
        if found is None:
            continue
        if pending is not None:
            state = pending == "claim"  # the previous command had no answer: it stands
        pending = found
    if pending is not None:
        state = pending == "claim"
    return state


def derive_issue(issue: Issue, pulls: list[Pull]) -> tuple[str | None, str]:
    """The Status an open issue's card should have, and why; None keeps the card's own."""
    linked = sorted(
        pull.number for pull in pulls if issue.number in pull.closes or issue.number in pull.part_of
    )
    if linked:
        return IN_REVIEW, "open pull request " + ", ".join(f"#{n}" for n in linked)
    if issue.assignees:
        return IN_PROGRESS, "assigned to " + ", ".join(issue.assignees)
    if claimed_in_comments(issue.comments):
        return IN_PROGRESS, "claimed in a comment"
    return None, ""


def plan(board: Board, issues: list[Issue], pulls: list[Pull], slug: str) -> list[Action]:
    """Every add and move that brings the board up to date, in issue and pull request order."""
    actions: list[Action] = []
    by_issue = {c.number: c for c in board.cards if c.repo == slug and c.kind == "Issue"}
    by_pull = {c.number: c for c in board.cards if c.repo == slug and c.kind == "PullRequest"}
    open_issues = {issue.number for issue in issues}

    def move(card: Card, content: str, status: str, reason: str) -> None:
        if same(card.status, status):
            return
        if same(card.status, HELD) and status == IN_PROGRESS:
            return  # a hold is a maintainer's decision; only a pull request or closing moves it
        actions.append(
            Action("move", content, card.number, card.title, status, card.status, card.item_id,
                   reason=reason)
        )

    for issue in sorted(issues, key=lambda i: i.number):
        status, reason = derive_issue(issue, pulls)
        card = by_issue.get(issue.number)
        if card is None:
            actions.append(
                Action("add", "issue", issue.number, issue.title, status or BACKLOG,
                       node_id=issue.node_id, reason=reason or "not on the board")
            )
        elif status is not None:
            move(card, "issue", status, reason)

    # Cards for issues that are no longer open: closed issues are done.
    for number, card in sorted(by_issue.items()):
        if number not in open_issues and card.state == "CLOSED":
            move(card, "issue", DONE, "closed")

    for pull in sorted(pulls, key=lambda p: p.number):
        if pull.number not in by_pull:
            actions.append(
                Action("add", "pull request", pull.number, pull.title, BACKLOG,
                       node_id=pull.node_id, reason="not on the board")
            )
    for number, card in sorted(by_pull.items()):
        if card.merged:
            move(card, "pull request", DONE, "merged")
    return actions


# ---- Writing ---------------------------------------------------------------------------------


def apply(graphql, board: Board, actions: list[Action]) -> None:
    for action in actions:
        option = board.option(action.status)
        item = action.item_id
        if action.kind == "add":
            added = graphql(ADD_MUTATION, {"project": board.project_id, "content": action.node_id})
            item = ((added.get("addProjectV2ItemById") or {}).get("item") or {}).get("id")
            if not item:
                raise SyncError(f"could not add {action.content} #{action.number} to the board")
        graphql(
            SET_MUTATION,
            {"project": board.project_id, "item": item, "field": board.field_id, "option": option},
        )


def http_graphql(token: str):
    def call(query: str, variables: dict) -> dict:
        request = urllib.request.Request(
            API,
            data=json.dumps({"query": query, "variables": variables}).encode(),
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json",
                "User-Agent": "suhina-project-sync",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                answer = json.load(response)
        except urllib.error.HTTPError as error:
            raise SyncError(f"GitHub answered {error.code}: {error.read()[:300]!r}") from error
        except urllib.error.URLError as error:
            raise SyncError(f"could not reach GitHub: {error.reason}") from error
        if answer.get("errors"):
            messages = "; ".join(e.get("message", "?") for e in answer["errors"])
            raise SyncError(f"GitHub refused the query: {messages}")
        return answer.get("data") or {}

    return call


def report(actions: list[Action], applied: bool) -> str:
    adds = sum(1 for a in actions if a.kind == "add")
    moves = len(actions) - adds
    head = "Applied" if applied else "Dry run, nothing written"
    lines = [f"{head}: {adds} to add, {moves} to move."]
    lines += [action.describe() for action in actions]
    return "\n".join(lines)


def run(graphql, owner: str, number: int, slug: str, write: bool) -> str:
    repo_owner, _, repo = slug.partition("/")
    board = read_board(graphql, owner, number)
    issues = read_issues(graphql, repo_owner, repo)
    pulls = read_pulls(graphql, repo_owner, repo)
    actions = plan(board, issues, pulls, slug)
    # Every Status this run needs exists before anything is written.
    for status in {action.status for action in actions}:
        board.option(status)
    if write:
        apply(graphql, board, actions)
    return report(actions, write)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="print the changes (the default)")
    mode.add_argument("--apply", action="store_true", help="write the changes to the board")
    args = parser.parse_args(argv)

    token = os.environ.get("PROJECT_TOKEN", "")
    if not token:
        print("PROJECT_TOKEN is not set; see docs/claim-bot.md.", file=sys.stderr)
        return 1
    owner = os.environ.get("PROJECT_OWNER") or "suhina-daw"
    try:
        number = int(os.environ.get("PROJECT_NUMBER") or "1")
    except ValueError:
        print("PROJECT_NUMBER is not a number.", file=sys.stderr)
        return 1
    slug = os.environ.get("GITHUB_REPOSITORY") or "suhina-daw/suhina-daw"

    try:
        text = run(http_graphql(token), owner, number, slug, args.apply)
    except SyncError as error:
        print(f"Project sync failed: {error}", file=sys.stderr)
        return 1
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as out:
            out.write("## Project sync\n\n```text\n" + text + "\n```\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
