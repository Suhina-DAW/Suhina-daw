"""Check the claim bot: the script `.github/workflows/claim.yml` runs.

The bot moves issue cards on the project board, so a wrong branch would write to the board instead
of failing a test. This runs the script with mocked API clients instead: every command
(`/claim`, `/unclaim`, `/hold`, `/ready`) is answered for each state a card can be in — free, held,
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
        assignees: (input.assignees || []).map((login) => ({ login })),
      },
      comment: {
        body: input.body,
        author_association: input.association || "NONE",
        user: { login: "someone" },
      },
    },
  };
  global.github = {
    rest: {
      issues: {
        createComment: async (args) => state.comments.push(args.body),
        listComments: async () => ({
          data: ((input.issue && input.issue.comments) || []).map((text) => ({ body: text })),
        }),
        addAssignees: async (args) => state.assigned.push(...args.assignees),
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
