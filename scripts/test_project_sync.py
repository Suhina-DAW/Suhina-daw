"""Tests for project_sync.py: python3 -m unittest discover -s scripts -p 'test_*.py'

The answers below are recorded GraphQL responses, cut down to the cards and issues each test needs;
`Recorded` serves them in place of the network and keeps every mutation the script sends.
"""

import io
import os
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock

import project_sync as ps

SLUG = "suhina-daw/suhina-daw"
REPO = {"nameWithOwner": SLUG}
OTHER = {"nameWithOwner": "suhina-daw/website"}
OPTIONS = [
    {"id": "f75ad846", "name": "Backlog"},
    {"id": "61e4505c", "name": "Ready"},
    {"id": "47fc9ee4", "name": "In progress"},
    {"id": "df73e18b", "name": "In review"},
    {"id": "4c1b0e4d", "name": "Do not start yet"},
    {"id": "98236657", "name": "Done"},
]
BOT = {"__typename": "Bot", "login": "github-actions"}


def user(login="someone"):
    return {"__typename": "User", "login": login}


def item(item_id, status, content):
    return {
        "id": item_id,
        "fieldValueByName": {"name": status} if status else None,
        "content": content,
    }


def issue_content(number, state="OPEN", repo=REPO):
    return {
        "__typename": "Issue",
        "id": f"I_{number}",
        "number": number,
        "title": f"Issue {number}",
        "state": state,
        "repository": repo,
    }


def pull_content(number, state="OPEN", merged=False):
    return {
        "__typename": "PullRequest",
        "id": f"PR_{number}",
        "number": number,
        "title": f"Pull {number}",
        "state": state,
        "merged": merged,
        "repository": REPO,
    }


def issue(number, assignees=(), comments=()):
    return {
        "id": f"I_{number}",
        "number": number,
        "title": f"Issue {number}",
        "assignees": {"nodes": [{"login": login} for login in assignees]},
        "comments": {"nodes": list(comments)},
    }


def pull(number, body="", closes=()):
    return {
        "id": f"PR_{number}",
        "number": number,
        "title": f"Pull {number}",
        "body": body,
        "closingIssuesReferences": {"nodes": [{"number": n, "repository": REPO} for n in closes]},
    }


def page(nodes, cursor=None):
    return {"pageInfo": {"hasNextPage": cursor is not None, "endCursor": cursor}, "nodes": nodes}


class Recorded:
    """Serves recorded answers. `items`, `issues` and `pulls` are lists of pages."""

    def __init__(self, items, issues, pulls, options=OPTIONS, project=True):
        self.items, self.issues, self.pulls = items, issues, pulls
        self.options, self.project = options, project
        self.mutations = []

    @staticmethod
    def _page(pages, after):
        index = 0 if after is None else int(after)
        return pages[index]

    def __call__(self, query, variables):
        after = variables.get("after")
        if "organization(login" in query:
            if not self.project:
                return {"organization": {"projectV2": None}}
            return {
                "organization": {
                    "projectV2": {
                        "id": "PVT_kwDOC",
                        "field": {"id": "PVTSSF_status", "options": self.options},
                        "items": self._page(self.items, after),
                    }
                }
            }
        if "issues(first" in query:
            return {"repository": {"issues": self._page(self.issues, after)}}
        if "pullRequests(first" in query:
            return {"repository": {"pullRequests": self._page(self.pulls, after)}}
        if "addProjectV2ItemById" in query:
            self.mutations.append(("add", variables["content"]))
            return {"addProjectV2ItemById": {"item": {"id": f"NEW_{variables['content']}"}}}
        if "updateProjectV2ItemFieldValue" in query:
            self.mutations.append(("set", variables["item"], variables["option"]))
            return {"updateProjectV2ItemFieldValue": {"projectV2Item": {"id": variables["item"]}}}
        raise AssertionError(f"unexpected query: {query[:60]}")


def moves(api, write=False):
    """The planned actions as (kind, number, status) for compact assertions."""
    board = ps.read_board(api, "suhina-daw", 1)
    actions = ps.plan(
        board,
        ps.read_issues(api, "suhina-daw", "suhina-daw"),
        ps.read_pulls(api, "suhina-daw", "suhina-daw"),
        SLUG,
    )
    if write:
        ps.apply(api, board, actions)
    return [(a.kind, a.number, a.status) for a in actions]


class Statuses(unittest.TestCase):
    def test_open_pull_request_that_closes_moves_to_review(self):
        api = Recorded(
            [page([item("ITEM_10", "In progress", issue_content(10))])],
            [page([issue(10, assignees=["ana"])])],
            [page([pull(20, closes=[10]), ])],
        )
        self.assertEqual(
            moves(api), [("move", 10, "In review"), ("add", 20, "Backlog")]
        )

    def test_part_of_in_the_body_counts(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10)), item("ITEM_20", "Backlog", pull_content(20))])],
            [page([issue(10)])],
            [page([pull(20, body="Part of #10, #11 and #12.")])],
        )
        self.assertEqual(moves(api), [("move", 10, "In review")])

    def test_closing_keyword_in_the_body_counts(self):
        api = Recorded(
            [page([item("ITEM_10", "Backlog", issue_content(10)), item("ITEM_20", None, pull_content(20))])],
            [page([issue(10)])],
            [page([pull(20, body="Fixes #10")])],
        )
        self.assertEqual(moves(api), [("move", 10, "In review")])

    def test_assignee_without_pull_request_is_in_progress(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10))])],
            [page([issue(10, assignees=["ana"])])],
            [page([])],
        )
        self.assertEqual(moves(api), [("move", 10, "In progress")])

    def test_claim_comment_without_bot_answer_is_in_progress(self):
        api = Recorded(
            [page([item("ITEM_10", "Backlog", issue_content(10))])],
            [page([issue(10, comments=[
                {"body": "Looks good", "author": user()},
                {"body": "/claim Plan: ...", "author": user()},
                {"body": "Command noted, @someone — but the board has no `PROJECT_TOKEN`", "author": BOT},
            ])])],
            [page([])],
        )
        self.assertEqual(moves(api), [("move", 10, "In progress")])

    def test_unclaim_after_claim_keeps_status(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10))])],
            [page([issue(10, comments=[
                {"body": "/claim", "author": user()},
                {"body": "/unclaim", "author": user()},
            ])])],
            [page([])],
        )
        self.assertEqual(moves(api), [])

    def test_refused_claim_does_not_count(self):
        api = Recorded(
            [page([item("ITEM_10", "Backlog", issue_content(10))])],
            [page([issue(10, comments=[
                {"body": "/claim", "author": user()},
                {"body": "This feature is not agreed yet (**Backlog**)", "author": BOT},
            ])])],
            [page([])],
        )
        self.assertEqual(moves(api), [])

    def test_refused_unclaim_keeps_the_claim(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10))])],
            [page([issue(10, comments=[
                {"body": "/claim", "author": user("ana")},
                {"body": "Claimed by @ana: **In progress**", "author": BOT},
                {"body": "/unclaim", "author": user("bo")},
                {"body": "Only the person who claimed this (@ana) or a maintainer can release it.", "author": BOT},
            ])])],
            [page([])],
        )
        self.assertEqual(moves(api), [("move", 10, "In progress")])

    def test_held_card_is_not_moved_by_a_claim(self):
        api = Recorded(
            [page([item("ITEM_10", "Do not start yet", issue_content(10))])],
            [page([issue(10, assignees=["ana"])])],
            [page([])],
        )
        self.assertEqual(moves(api), [])

    def test_held_card_is_moved_by_a_pull_request(self):
        api = Recorded(
            [page([item("ITEM_10", "Do not start yet", issue_content(10)), item("ITEM_20", "Backlog", pull_content(20))])],
            [page([issue(10)])],
            [page([pull(20, closes=[10])])],
        )
        self.assertEqual(moves(api), [("move", 10, "In review")])

    def test_closed_issue_and_merged_pull_request_are_done(self):
        api = Recorded(
            [page([
                item("ITEM_10", "In progress", issue_content(10, state="CLOSED")),
                item("ITEM_11", "Done", issue_content(11, state="CLOSED")),
                item("ITEM_20", "In review", pull_content(20, state="MERGED", merged=True)),
                item("ITEM_21", "In review", pull_content(21, state="CLOSED")),
            ])],
            [page([])],
            [page([])],
        )
        self.assertEqual(moves(api), [("move", 10, "Done"), ("move", 20, "Done")])

    def test_everything_else_keeps_its_status(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10)), item("ITEM_11", "Done", issue_content(11))])],
            [page([issue(10), issue(11)])],
            [page([])],
        )
        self.assertEqual(moves(api), [])

    def test_status_matches_without_case(self):
        api = Recorded(
            [page([item("ITEM_10", "In Progress", issue_content(10))])],
            [page([issue(10, assignees=["ana"])])],
            [page([])],
        )
        self.assertEqual(moves(api), [])

    def test_other_repositories_and_drafts_are_left_alone(self):
        api = Recorded(
            [page([
                item("ITEM_1", "Backlog", issue_content(10, state="CLOSED", repo=OTHER)),
                item("ITEM_2", "Backlog", {"__typename": "DraftIssue"}),
                item("ITEM_3", "Backlog", None),
            ])],
            [page([])],
            [page([])],
        )
        self.assertEqual(moves(api), [])


class Adding(unittest.TestCase):
    def test_missing_cards_are_added_with_a_derived_status(self):
        api = Recorded(
            [page([])],
            [page([issue(10), issue(11, assignees=["ana"]), issue(12)])],
            [page([pull(20, closes=[12])])],
        )
        self.assertEqual(
            moves(api),
            [
                ("add", 10, "Backlog"),
                ("add", 11, "In progress"),
                ("add", 12, "In review"),
                ("add", 20, "Backlog"),
            ],
        )

    def test_every_page_is_read(self):
        api = Recorded(
            [page([item("ITEM_10", "Backlog", issue_content(10))], cursor="1"),
             page([item("ITEM_11", "Backlog", issue_content(11))])],
            [page([issue(10)], cursor="1"), page([issue(11)], cursor="2"), page([issue(12)])],
            [page([])],
        )
        self.assertEqual(moves(api), [("add", 12, "Backlog")])


class Writing(unittest.TestCase):
    def test_apply_adds_then_sets_status_and_touches_nothing_else(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10))])],
            [page([issue(10, assignees=["ana"]), issue(11)])],
            [page([])],
        )
        moves(api, write=True)
        self.assertEqual(
            api.mutations,
            [
                ("set", "ITEM_10", "47fc9ee4"),
                ("add", "I_11"),
                ("set", "NEW_I_11", "f75ad846"),
            ],
        )

    def test_dry_run_writes_nothing(self):
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10))])],
            [page([issue(10, assignees=["ana"]), issue(11)])],
            [page([])],
        )
        text = ps.run(api, "suhina-daw", 1, SLUG, write=False)
        self.assertEqual(api.mutations, [])
        self.assertIn("Dry run, nothing written: 1 to add, 1 to move.", text)
        self.assertIn("move  issue #10 Issue 10: Ready -> In progress (assigned to ana)", text)
        self.assertIn("add   issue #11 Issue 11 -> Backlog (not on the board)", text)

    def test_missing_option_stops_before_writing(self):
        options = [o for o in OPTIONS if o["name"] != "In review"]
        api = Recorded(
            [page([item("ITEM_10", "Ready", issue_content(10))])],
            [page([issue(10), issue(11)])],
            [page([pull(20, closes=[10])])],
            options=options,
        )
        with self.assertRaisesRegex(ps.SyncError, "no option 'In review'"):
            ps.run(api, "suhina-daw", 1, SLUG, write=True)
        self.assertEqual(api.mutations, [])

    def test_invisible_project_is_an_error(self):
        api = Recorded([page([])], [page([])], [page([])], project=False)
        with self.assertRaisesRegex(ps.SyncError, "visible to PROJECT_TOKEN"):
            ps.run(api, "suhina-daw", 1, SLUG, write=False)


class Command(unittest.TestCase):
    def test_without_token_fails(self):
        err = io.StringIO()
        with mock.patch.dict(os.environ, {"PROJECT_TOKEN": ""}), redirect_stderr(err):
            self.assertEqual(ps.main([]), 1)
        self.assertIn("PROJECT_TOKEN", err.getvalue())

    def test_default_is_a_dry_run(self):
        api = Recorded([page([])], [page([issue(10)])], [page([])])
        out = io.StringIO()
        with mock.patch.dict(os.environ, {"PROJECT_TOKEN": "t", "GITHUB_STEP_SUMMARY": ""}), \
                mock.patch.object(ps, "http_graphql", return_value=api), redirect_stdout(out):
            self.assertEqual(ps.main([]), 0)
        self.assertEqual(api.mutations, [])
        self.assertIn("Dry run", out.getvalue())

    def test_apply_writes(self):
        api = Recorded([page([])], [page([issue(10)])], [page([])])
        with mock.patch.dict(os.environ, {"PROJECT_TOKEN": "t", "GITHUB_STEP_SUMMARY": ""}), \
                mock.patch.object(ps, "http_graphql", return_value=api), redirect_stdout(io.StringIO()):
            self.assertEqual(ps.main(["--apply"]), 0)
        self.assertEqual(api.mutations, [("add", "I_10"), ("set", "NEW_I_10", "f75ad846")])


class References(unittest.TestCase):
    def test_reads_closes_and_part_of(self):
        self.assertEqual(
            ps.references("Closes #3. Part of #4 and #5.\nresolved #6"),
            ({3, 6}, {4, 5}),
        )

    def test_ignores_plain_mentions(self):
        self.assertEqual(ps.references("See #3; follows #4"), (set(), set()))


if __name__ == "__main__":
    unittest.main()
