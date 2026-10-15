"""Check the claim bot: the script `.github/workflows/claim.yml` runs.

The bot moves issue cards on the project board, so a wrong branch would write to the board instead
of failing a test. This runs the script with mocked API clients instead: every command
(`/claim`, `/claim @login`, `/unclaim`, `/hold`, `/ready`) is answered for each state a card can be in — free, held,
taken, closed, absent from the board — and for the mistakes the bot is meant to catch (an open pull
request, an epic, a status the field does not have, a missing token).

    python scripts/claim_bot_test.py

Needs Node.js on the PATH, which the GitHub runner has. The two files it runs are written under
`target/claim-bot-test/` and removed afterwards; nothing else is touched.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "claim.yml"
# Under `target/`, which the build already ignores: a system temporary directory is not writable
# everywhere this runs.
WORK = ROOT / "target" / "claim-bot-test"

# The harness: `claim-bot.js` is the workflow's script, wrapped and exported by this test.
HARNESS = r'''// Runs the claim bot's script against mocked API clients. Written by scripts/claim_bot_test.py.
const main = require("./claim-bot.js");

const OPTIONS = [
  { id: "opt-backlog", name: "Backlog" },
  { id: "opt-ready", name: "Ready" },
  { id: "opt-progress", name: "In progress" },
  { id: "opt-review", name: "In review" },
  { id: "opt-hold", name: "Do not start yet" },
  { id: "opt-done", name: "Done" },
];

function scenario(input) {
  const state = { moved: undefined, added: false, assigned: [], unassigned: [], comments: [] };
  const board = input.board || {};
  const graphql = async (query, vars) => {
    if (query.includes("organization(login")) {
      return {
        organization: {
          projectV2: {
            id: "PVT_1",
            field: {
              id: "PVTSSF_1",
              options: board.renamed
                ? OPTIONS.filter((option) => option.name !== "In progress")
                : OPTIONS,
            },
          },
        },
        node: {
          projectItems: {
            nodes:
              board.onBoard === false
                ? []
                : [
                    {
                      id: "ITEM_1",
                      project: { id: "PVT_1" },
                      fieldValueByName: board.status ? { name: board.status } : null,
                    },
                  ],
          },
        },
      };
    }
    if (query.includes("closedByPullRequestsReferences")) {
      return { node: { closedByPullRequestsReferences: { nodes: board.openPrs || [] } } };
    }
    if (query.includes("addProjectV2ItemById")) {
      state.added = true;
      return { addProjectV2ItemById: { item: { id: "ITEM_NEW" } } };
    }
    if (query.includes("updateProjectV2ItemFieldValue")) {
      state.moved = vars.option;
      return { updateProjectV2ItemFieldValue: { projectV2Item: { id: vars.item } } };
    }
    throw new Error(`unexpected query: ${query.slice(0, 60)}`);
  };

  process.env.PROJECT_OWNER = "suhina-daw";
  process.env.PROJECT_NUMBER = "1";
  process.env.PROJECT_TOKEN = input.token === undefined ? "token" : input.token;
  global.getOctokit = () => ({ graphql });
  global.core = { warning: () => {}, setFailed: (message) => (state.failed = message) };
  global.context = {
    repo: { owner: "suhina-daw", repo: "suhina-daw" },
    serverUrl: "https://github.com",
    payload: {
      issue: {
        number: 42,
        node_id: "I_42",
        labels: (input.issue && input.issue.labels) || [],
        // The event's copy, which can be older than the issue (`stale`).
        assignees: (input.stale || input.assignees || []).map((login) => ({ login })),
      },
      comment: {
        body: input.body,
        author_association: input.association || "NONE",
        user: { login: "someone" },
      },
    },
  };
  // A comment is its text, written by a maintainer, or { body, by } for anyone else; { bot, hoursAgo }
  // makes it the bot's own comment, written that many hours ago.
  const listComments = async () => ({
    data: ((input.issue && input.issue.comments) || []).map((entry) =>
      typeof entry === "string"
        ? { body: entry, author_association: "MEMBER", user: { type: "User" } }
        : {
            body: entry.body,
            author_association: entry.bot ? "NONE" : entry.by,
            user: { type: entry.bot ? "Bot" : "User" },
            created_at: new Date(Date.now() - (entry.hoursAgo || 0) * 3600 * 1000).toISOString(),
          },
    ),
  });
  global.github = {
    paginate: async (method, args) => (await method(args)).data,
    rest: {
      issues: {
        createComment: async (args) => state.comments.push(args.body),
        get: async () => ({
          data: {
            number: 42,
            labels: (input.issue && input.issue.labels) || [],
            assignees: (input.assignees || []).map((login) => ({ login })),
          },
        }),
        listComments,
        // GitHub answers with the issue, and silently leaves out anyone it may not assign.
        addAssignees: async (args) => {
          const taken = args.assignees.filter((login) => !(input.unassignable || []).includes(login));
          state.assigned.push(...taken);
          const all = [...(input.assignees || []), ...taken];
          return { data: { assignees: all.map((login) => ({ login })) } };
        },
        removeAssignees: async (args) => state.unassigned.push(...args.assignees),
      },
    },
  };
  return { run: main, state };
}

const says = (state, ...parts) => state.comments.some((c) => parts.every((part) => c.includes(part)));

const cases = [
  {
    name: "claim on a free card moves it and assigns the claimer",
    input: { body: "/claim Plan: fix the wheel routing", board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by") && s.state.assigned.includes("someone"),
  },
  {
    name: "a card in Backlog is free as well",
    input: { body: "/claim", board: { status: "Backlog" } },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "claim on an issue that is not on the board adds it first",
    input: { body: "/claim", board: { onBoard: false } },
    expect: (s) => s.state.added && s.state.moved === "opt-progress",
  },
  {
    name: "claim while someone else works is refused",
    input: { body: "/claim", board: { status: "In progress" }, assignees: ["other"] },
    expect: (s) => says(s.state, "already **In progress** (@other)") && !s.state.moved,
  },
  {
    name: "a card in review counts as someone working on it",
    input: { body: "/claim", board: { status: "In review" }, assignees: ["other"] },
    expect: (s) => says(s.state, "already **In review** (@other)") && !s.state.moved,
  },
  {
    name: "claim by the person who already has it is named as theirs",
    input: { body: "/claim", board: { status: "In progress" }, assignees: ["someone"] },
    expect: (s) => says(s.state, "already yours") && !s.state.moved,
  },
  {
    name: "claim by the person who has it names their open pull request",
    input: {
      body: "/claim",
      board: { status: "In progress", openPrs: [{ number: 239, title: "Fix", url: "u" }] },
      assignees: ["someone"],
    },
    expect: (s) => says(s.state, "already yours", "#239", "Continue there") && !s.state.moved,
  },
  {
    name: "a claim with no pull request after 48 hours passes to the next claimer",
    input: {
      body: "/claim",
      board: { status: "In progress" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 49 }] },
    },
    expect: (s) =>
      says(s.state, "lapsed", "@other") &&
      s.state.unassigned.includes("other") &&
      s.state.assigned.includes("someone") &&
      s.state.moved === "opt-progress",
  },
  {
    name: "a claim younger than 48 hours still holds",
    input: {
      body: "/claim",
      board: { status: "In progress" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 10 }] },
    },
    expect: (s) => says(s.state, "already **In progress** (@other)") && !s.state.moved && !s.state.unassigned.length,
  },
  {
    name: "an old claim with an open pull request does not lapse",
    input: {
      body: "/claim",
      board: { status: "In progress", openPrs: [{ number: 239, title: "Fix", url: "u" }] },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 200 }] },
    },
    expect: (s) => !says(s.state, "lapsed") && !s.state.moved && !s.state.unassigned.length,
  },
  {
    name: "only the bot's own claim comment dates a claim",
    input: {
      body: "/claim",
      board: { status: "In progress" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: forged", by: "NONE", hoursAgo: 200 }] },
    },
    expect: (s) => says(s.state, "already **In progress**") && !s.state.moved,
  },
  {
    name: "a card in review never lapses",
    input: {
      body: "/claim",
      board: { status: "In review" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 200 }] },
    },
    expect: (s) => says(s.state, "already **In review**") && !s.state.moved,
  },
  {
    name: "claim on a held card quotes the reason it was held for",
    input: {
      body: "/claim",
      board: { status: "Do not start yet" },
      issue: { comments: ["/hold waiting for ADR 0003"] },
    },
    expect: (s) => says(s.state, "This is held", "waiting for ADR 0003") && !s.state.moved,
  },
  {
    name: "claim with an open pull request is refused, and names it",
    input: { body: "/claim", board: { status: "Ready", openPrs: [{ number: 239, title: "Fix", url: "u" }] } },
    expect: (s) => says(s.state, "already a pull request", "#239") && !s.state.moved,
  },
  {
    name: "a feature still in Backlog is not open to claim",
    input: { body: "/claim", board: { status: "Backlog" }, issue: { labels: [{ name: "enhancement" }] } },
    expect: (s) => says(s.state, "not agreed yet") && !s.state.moved,
  },
  {
    name: "a feature in Ready can be claimed",
    input: { body: "/claim", board: { status: "Ready" }, issue: { labels: [{ name: "enhancement" }] } },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a maintainer may claim a feature in Backlog",
    input: {
      body: "/claim",
      association: "MEMBER",
      board: { status: "Backlog" },
      issue: { labels: [{ name: "enhancement" }] },
    },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a maintainer claims for someone else, who is assigned and named",
    input: { body: "/claim @Other-Person Plan: the wheel routing", association: "MEMBER", board: { status: "Ready" } },
    expect: (s) =>
      s.state.moved === "opt-progress" &&
      s.state.assigned.length === 1 &&
      s.state.assigned[0] === "Other-Person" &&
      says(s.state, "Claimed by @Other-Person, on behalf of a maintainer (@someone)"),
  },
  {
    name: "a maintainer claims for someone assigned by hand: the card moves, nobody is assigned again",
    input: { body: "/claim @other", association: "OWNER", assignees: ["Other"], board: { status: "Ready" } },
    expect: (s) =>
      s.state.moved === "opt-progress" && s.state.assigned.length === 0 && says(s.state, "Claimed by @Other, on behalf"),
  },
  {
    name: "a maintainer may claim a feature in Backlog for someone",
    input: {
      body: "/claim @other",
      association: "COLLABORATOR",
      board: { status: "Backlog" },
      issue: { labels: [{ name: "enhancement" }] },
    },
    expect: (s) => s.state.moved === "opt-progress" && s.state.assigned.includes("other"),
  },
  {
    name: "a maintainer's claim for someone already In progress for them changes nothing",
    input: { body: "/claim @other", association: "MEMBER", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "already **In progress** for @other") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a contributor may not claim for someone else",
    input: { body: "/claim @other Plan: …", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer can claim an issue for someone else") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a contributor naming themselves makes an ordinary claim",
    input: { body: "/claim @Someone Plan: …", board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by @someone:") && !says(s.state, "on behalf"),
  },
  {
    name: "a name that is not a GitHub login is refused and not quoted",
    input: { body: "/claim @bad$(name) @everyone", association: "MEMBER", board: { status: "Ready" } },
    expect: (s) =>
      says(s.state, "not a GitHub username") && !says(s.state, "bad$") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a login with a double hyphen is refused",
    input: { body: `/claim @a--b`, association: "MEMBER", board: { status: "Ready" } },
    expect: (s) => says(s.state, "not a GitHub username") && !s.state.moved,
  },
  {
    name: "a login longer than 39 characters is refused",
    input: { body: `/claim @${"a".repeat(40)}`, association: "MEMBER", board: { status: "Ready" } },
    expect: (s) => says(s.state, "not a GitHub username") && !s.state.moved,
  },
  {
    name: "a trailing comma after the name is not part of it",
    input: { body: "/claim @other, please take this", association: "MEMBER", board: { status: "Ready" } },
    expect: (s) => s.state.assigned.includes("other") && s.state.moved === "opt-progress",
  },
  {
    name: "a maintainer's claim for someone on a held card is refused",
    input: {
      body: "/claim @other",
      association: "MEMBER",
      board: { status: "Do not start yet" },
      issue: { comments: ["/hold waiting for ADR 0003"] },
    },
    expect: (s) => says(s.state, "This is held", "waiting for ADR 0003") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a maintainer's claim for someone on a taken card is refused",
    input: { body: "/claim @other", association: "MEMBER", assignees: ["third"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "already **In progress** (@third)") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a maintainer's claim for someone is refused when a pull request closes the issue",
    input: {
      body: "/claim @other",
      association: "MEMBER",
      board: { status: "Ready", openPrs: [{ number: 239, title: "Fix", url: "u" }] },
    },
    expect: (s) => says(s.state, "already a pull request", "#239") && !s.state.moved,
  },
  {
    name: "a maintainer's claim for someone on an epic is refused",
    input: { body: "/claim @other", association: "MEMBER", board: { status: "Ready" }, issue: { labels: [{ name: "epic" }] } },
    expect: (s) => says(s.state, "`epic`") && !s.state.moved,
  },
  {
    name: "a lapsed claim passes to the person a maintainer names",
    input: {
      body: "/claim @other",
      association: "MEMBER",
      board: { status: "In progress" },
      assignees: ["third"],
      issue: { comments: [{ body: "Claimed by @third: **In progress**", bot: true, hoursAgo: 49 }] },
    },
    expect: (s) =>
      says(s.state, "lapsed", "passes to @other") &&
      s.state.unassigned.includes("third") &&
      s.state.assigned.includes("other") &&
      s.state.moved === "opt-progress",
  },
  {
    name: "someone GitHub will not assign is named in the answer",
    input: { body: "/claim @ghost", association: "MEMBER", unassignable: ["ghost"], board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by @ghost", "GitHub did not assign @ghost"),
  },
  {
    name: "/unclaim ignores a name after it",
    input: { body: "/unclaim @other", assignees: ["someone"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("someone"),
  },
  {
    name: "claim on an epic is refused",
    input: { body: "/claim", board: { status: "Ready" }, issue: { labels: [{ name: "epic" }] } },
    expect: (s) => says(s.state, "`epic`") && !s.state.moved,
  },
  {
    name: "hold by a contributor is refused",
    input: { body: "/hold because I say so", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer") && !s.state.moved,
  },
  {
    name: "hold without a reason is refused",
    input: { body: "/hold", association: "MEMBER", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Write the reason") && !s.state.moved,
  },
  {
    name: "hold with a reason gates the card",
    input: { body: "/hold\nwaiting for ADR 0003", association: "MEMBER", board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-hold" && says(s.state, "Held"),
  },
  {
    name: "ready opens a held card again",
    input: { body: "/ready", association: "OWNER", board: { status: "Do not start yet" } },
    expect: (s) => s.state.moved === "opt-ready" && says(s.state, "Released"),
  },
  {
    name: "unclaim by the assignee releases and unassigns",
    input: { body: "/unclaim", assignees: ["someone"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("someone"),
  },
  {
    name: "unclaim by someone else is refused",
    input: { body: "/unclaim", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "Only the person who claimed") && !s.state.moved,
  },
  {
    name: "unclaim on a held card unassigns but keeps the hold",
    input: { body: "/unclaim", assignees: ["someone"], board: { status: "Do not start yet" } },
    expect: (s) => !s.state.moved && s.state.unassigned.includes("someone") && says(s.state, "stays **Do not start yet**"),
  },
  {
    name: "unclaim on a card in review unassigns but keeps it in review",
    input: { body: "/unclaim", assignees: ["someone"], board: { status: "In review" } },
    expect: (s) => !s.state.moved && says(s.state, "stays **In review**"),
  },
  {
    name: "unclaim by a maintainer releases the person who claimed it",
    input: { body: "/unclaim", association: "MEMBER", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("other") && !s.state.unassigned.includes("someone"),
  },
  {
    name: "unclaim on a free card with nobody assigned says it is free",
    input: { body: "/unclaim", association: "MEMBER", board: { status: "Ready" } },
    expect: (s) => says(s.state, "already free") && !s.state.moved,
  },
  {
    name: "unclaim reads the assignees again, not the event's old copy",
    input: { body: "/unclaim", stale: [], assignees: ["someone"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("someone"),
  },
  {
    name: "a card without a status can be claimed",
    input: { body: "/claim", board: {} },
    expect: (s) => s.state.moved === "opt-progress" && !says(s.state, "null"),
  },
  {
    name: "a hold reason is quoted only from a maintainer, without mentions",
    input: {
      body: "/claim",
      board: { status: "Do not start yet" },
      issue: {
        comments: [
          "/hold waiting for @someone and ADR 0003",
          { body: "/hold @everyone go to evil.example", by: "NONE" },
        ],
      },
    },
    expect: (s) =>
      says(s.state, "waiting for someone and ADR 0003") && !says(s.state, "evil.example") && !s.state.moved,
  },
  {
    name: "a comment that only mentions a command is ignored",
    input: { body: "I think /claim would be nice here", board: { status: "Ready" } },
    expect: (s) => s.state.comments.length === 0 && !s.state.moved,
  },
  {
    name: "a missing token is announced instead of failing silently",
    input: { body: "/claim", token: "", board: { status: "Ready" } },
    expect: (s) => says(s.state, "PROJECT_TOKEN"),
  },
  {
    name: "a renamed status names the options the field has",
    input: { body: "/claim", board: { status: "Ready", renamed: true } },
    expect: (s) => says(s.state, "has no **In progress** option", "Do not start yet") && !s.state.moved,
  },
];

(async () => {
  let failed = 0;
  for (const item of cases) {
    const s = scenario(item.input);
    let error = null;
    try {
      await s.run();
    } catch (thrown) {
      error = thrown;
    }
    const ok = !error && item.expect(s);
    if (!ok) failed += 1;
    console.log(`${ok ? "PASS" : "FAIL"}  ${item.name}`);
    if (!ok && error) console.log(`       threw: ${error.message}`);
  }
  console.log(failed === 0 ? `${cases.length}/${cases.length} branches answered as intended` : `${failed} failed`);
  process.exit(failed === 0 ? 0 : 1);
})();
'''


def job_script(text: str) -> str:
    """The workflow's one `script: |` block, read without a YAML parser.

    CI installs nothing but the checkout and Node.js, so the block is taken from the text: its
    indentation is the first non-empty line's, and it ends at the first line that is not part of it.
    """
    lines = text.splitlines()
    starts = [at for at, line in enumerate(lines) if line.strip() == "script: |"]
    if len(starts) != 1:
        raise SystemExit(f"expected exactly one `script: |` block, found {len(starts)}")
    block: list[str] = []
    indent: int | None = None
    for line in lines[starts[0] + 1 :]:
        if not line.strip():
            block.append("")
            continue
        width = len(line) - len(line.lstrip(" "))
        if indent is None:
            indent = width
        if width < indent:
            break
        block.append(line[indent:])
    while block and not block[-1]:
        block.pop()
    return "\n".join(block)


def node(*args: str, cwd: pathlib.Path) -> int:
    try:
        done = subprocess.run(["node", *args], cwd=cwd, capture_output=True, text=True)
    except FileNotFoundError:
        print("node is not on the PATH; the bot's script runs on Node.js")
        return 1
    if done.stdout:
        print(done.stdout, end="")
    if done.returncode != 0 and done.stderr:
        print(done.stderr, end="", file=sys.stderr)
    return done.returncode


def main() -> int:
    script = job_script(WORKFLOW.read_text(encoding="utf-8"))
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)
    try:
        (WORK / "claim-bot.js").write_text(
            f"async function main() {{\n{script}\n}}\nmodule.exports = main;\n", encoding="utf-8"
        )
        (WORK / "claim-bot.test.js").write_text(HARNESS, encoding="utf-8")
        if node("--check", "claim-bot.js", cwd=WORK) != 0:
            print("the workflow's script does not parse")
            return 1
        return node("claim-bot.test.js", cwd=WORK)
    finally:
        shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
