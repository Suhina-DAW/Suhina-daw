"""Check the `Depends on` check: the script `.github/workflows/depends-on.yml` runs.

The check decides whether the merge queue may merge a pull request, so it is run here against
mocked API clients for every way a description can name a dependency (`Depends on #N`,
`Depends-On: #N`, a link, a bullet, an HTML comment that must be ignored) and every state the
dependency can be in (an open, merged or closed pull request, an open or closed issue, one that
cannot be read), on a pull request and in the merge queue.

    python scripts/depends_on_test.py

Needs Node.js on the PATH, which the GitHub runner has. The two files it runs are written under
`target/depends-on-test/` and removed afterwards; nothing else is touched.
"""

from __future__ import annotations

import pathlib
import shutil
import sys

from claim_bot_test import job_script, node

ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "depends-on.yml"
WORK = ROOT / "target" / "depends-on-test"

HARNESS = r'''// Runs the Depends on check against mocked API clients. Written by scripts/depends_on_test.py.
const main = require("./depends-on.js");

// What the API knows: number -> { title, state, pr, merged }.
function scenario(input) {
  const state = { failed: null, read: [] };
  const items = input.items || {};
  const summary = {
    addHeading() { return summary; },
    addList(list) { state.listed = list; return summary; },
    addRaw() { return summary; },
    async write() { return summary; },
  };
  global.core = {
    info: () => {},
    setFailed: (message) => (state.failed = message),
    summary,
  };
  global.context = {
    repo: { owner: "suhina-daw", repo: "suhina-daw" },
    payload: input.queue
      ? { merge_group: { head_ref: input.queue } }
      : { pull_request: { number: 100, body: input.body } },
  };
  global.github = {
    rest: {
      pulls: {
        get: async (args) => ({ data: { number: args.pull_number, body: input.body } }),
      },
      issues: {
        get: async (args) => {
          state.read.push(args.issue_number);
          const item = items[args.issue_number];
          if (!item) throw new Error("Not Found");
          return {
            data: {
              number: args.issue_number,
              title: item.title || "Work",
              state: item.state,
              pull_request: item.pr ? { merged_at: item.merged ? "2026-10-15T00:00:00Z" : null } : undefined,
            },
          };
        },
      },
    },
  };
  return { run: main, state };
}

const passes = (s) => s.state.failed === null;
const fails = (s, ...parts) => s.state.failed !== null && parts.every((part) => s.state.failed.includes(part));

const cases = [
  {
    name: "a description without a dependency passes and reads nothing",
    input: { body: "Closes #5" },
    expect: (s) => passes(s) && s.state.read.length === 0,
  },
  {
    name: "an open pull request it depends on fails the check, and is named",
    input: { body: "Depends on #7", items: { 7: { title: "Model field", state: "open", pr: true } } },
    expect: (s) => fails(s, "#7 Model field", "not merged yet"),
  },
  {
    name: "a merged pull request lets it pass",
    input: { body: "Depends on #7", items: { 7: { state: "closed", pr: true, merged: true } } },
    expect: passes,
  },
  {
    name: "a pull request closed without merging still blocks",
    input: { body: "Depends on #7", items: { 7: { state: "closed", pr: true } } },
    expect: (s) => fails(s, "closed without merging"),
  },
  {
    name: "an open issue blocks and a closed one does not",
    input: {
      body: "Depends on #8\nDepends on #9",
      items: { 8: { title: "Open", state: "open" }, 9: { title: "Done", state: "closed" } },
    },
    expect: (s) => fails(s, "#8 Open", "still open") && !s.state.failed.includes("#9"),
  },
  {
    name: "Depends-On:, a bullet and a link to this repository are read",
    input: {
      body: "Depends-On: #1\n- depends on https://github.com/suhina-daw/suhina-daw/pull/2\n* Depends on #3",
      items: { 1: { state: "open", pr: true }, 2: { state: "open", pr: true }, 3: { state: "open" } },
    },
    expect: (s) => fails(s, "#1", "#2", "#3"),
  },
  {
    name: "a link to another repository is not read",
    input: { body: "Depends on https://github.com/other/repo/pull/2" },
    expect: (s) => passes(s) && s.state.read.length === 0,
  },
  {
    name: "a line inside an HTML comment is not a dependency",
    input: { body: "<!-- Depends on #7 -->\nCloses #5", items: { 7: { state: "open", pr: true } } },
    expect: (s) => passes(s) && s.state.read.length === 0,
  },
  {
    name: "mentioning a dependency mid-sentence is not one",
    input: { body: "This no longer depends on #7.", items: { 7: { state: "open", pr: true } } },
    expect: (s) => passes(s) && s.state.read.length === 0,
  },
  {
    name: "a pull request naming itself is ignored",
    input: { body: "Depends on #100", items: { 100: { state: "open", pr: true } } },
    expect: (s) => passes(s) && s.state.read.length === 0,
  },
  {
    name: "a dependency that cannot be read fails rather than passing",
    input: { body: "Depends on #404" },
    expect: (s) => fails(s, "#404", "could not be read"),
  },
  {
    name: "in the merge queue the pull request is found from the branch name",
    input: {
      queue: "refs/heads/gh-readonly-queue/main/pr-321-0123abcd",
      body: "Depends on #7",
      items: { 7: { state: "open", pr: true } },
    },
    expect: (s) => fails(s, "#321 waits for", "#7"),
  },
  {
    name: "a queue branch without a pull request number checks nothing",
    input: { queue: "refs/heads/something-else", body: "Depends on #7", items: { 7: { state: "open", pr: true } } },
    expect: (s) => passes(s) && s.state.read.length === 0,
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
  console.log(failed === 0 ? `${cases.length}/${cases.length} cases answered as intended` : `${failed} failed`);
  process.exit(failed === 0 ? 0 : 1);
})();
'''


def main() -> int:
    script = job_script(WORKFLOW.read_text(encoding="utf-8"))
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)
    try:
        (WORK / "depends-on.js").write_text(
            f"async function main() {{\n{script}\n}}\nmodule.exports = main;\n", encoding="utf-8"
        )
        (WORK / "depends-on.test.js").write_text(HARNESS, encoding="utf-8")
        if node("--check", "depends-on.js", cwd=WORK) != 0:
            print("the workflow's script does not parse")
            return 1
        return node("depends-on.test.js", cwd=WORK)
    finally:
        shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
