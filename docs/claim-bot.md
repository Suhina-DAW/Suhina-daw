---
description: How the claim bot moves an issue's card on the project board, holds work, and is set up
read_when: claim, /claim, /unclaim, /hold, /ready, claim bot, project board, PROJECT_TOKEN, status automation, good first issue
type: howto
status: current
last_verified: 2026-10-13
covers:
  - .github/workflows/claim.yml
  - scripts/claim_bot_test.py
---

# The claim bot

[`.github/workflows/claim.yml`](../.github/workflows/claim.yml) moves an issue's card on the
[Suhina project board](https://github.com/orgs/suhina-daw/projects/1) when someone comments one of
four commands, so that taking a task works the way [`CONTRIBUTING.md`](../CONTRIBUTING.md) ›
*Working on an issue* describes, without a maintainer moving cards by hand.

| Comment on an issue | What happens |
|---|---|
| `/claim` | Refused when the card is held, when an open pull request closes the issue, when the issue is an `epic`, or when the status says someone is already on it. Otherwise the issue is added to the board if it is not there, Status becomes `In progress`, and the commenter is assigned |
| `/unclaim` | By the assigned person, who is unassigned, or a maintainer, who unassigns whoever claimed it. A card `In progress` goes back to `Ready`; a held card or one `In review` keeps its status |
| `/hold <why>` | A maintainer only, and the reason is required: Status becomes `Do not start yet` |
| `/ready` | A maintainer only: Status goes back to `Ready`, so the card can be claimed again |
| anything else | Ignored; a comment that does not start with `/` does not even start a run |

Contributors keep working without it: a plain comment that says "I'd like to take this" is still a
claim, and the bot says so in the issue when it cannot do its job.

The bot reads the board rather than hard-coding it: it finds the project's `Status` field by name,
treats `Backlog`, `Ready` and a card without a status as free, `Do not start yet` as held, `In progress` and `In review` as
someone already on it, and every other value as not open to start; all of them are matched without
case. The names live at the top of the script, so renaming a status is a one-line change there;
until then the bot says so in the issue and names the options the field does have.

## Holding work

`Do not start yet` is neither a claim nor a failure: it says the order is deliberate — the work
waits for a decision, another issue or an unfinished spec. The reason belongs in the issue rather
than in the card, because the issue is what the next person, and the next agent, reads: a claim on a
held card is refused with the most recent `/hold` reason quoted back, so the gate explains itself.
Only a maintainer's `/hold` is quoted (anyone else's was refused), and without its `@` signs, so a
quote never pings anyone.

The board's `Status` field needs that option before the first `/hold` (Project → ⋯ → Settings →
Status → add `Do not start yet`). It has `Backlog`, `Ready`, `In progress`, `In review` and `Done`
today; until the gate exists, the bot names the options the field does have.

## Setting it up

The workflow's `GITHUB_TOKEN` has no access to a project owned by an organization, so the job needs
a token of its own, in the repository secret **`PROJECT_TOKEN`** (Settings → Secrets and variables →
Actions → New repository secret):

- **Fine-grained, recommended:** owned by the organization, with **Organization permissions →
  Projects: Read and write**, and on `suhina-daw/suhina-daw` the repository permissions
  **Issues: Read** and **Pull requests: Read**.
- **Classic:** the scopes `project` and `public_repo` (and `repo`, if the repository ever becomes
  private).

Both kinds expire, and a fine-grained token expires within a year. Put that date where the release
process keeps its reminders ([`releasing.md`](releasing.md)).

## When it fails

The bot's comment on the issue is the diagnosis: it says which part it could not do, and the run's
log is under Actions → Claim. Nothing else has to be repaired, because the bot keeps no state of
its own — a wrong card is fixed by hand on the board. Deleting the secret turns the bot off
completely: claims then work as plain comments, exactly as they did before it existed.

The open pull request check is a courtesy, not a lock: anyone who opens a pull request, a draft
included, that says `Closes #N` makes the bot refuse claims of that issue until it is closed. A
maintainer who finds such a pull request abandoned closes it, or moves the card by hand.

## How it is checked

```sh
python scripts/claim_bot_test.py
```

The script runs the workflow's script against mocked API clients, because a wrong branch would write
to the board instead of failing a test: every command is answered for each state a card can be in
(free, without a status, held, taken, in review, absent from the board), for `/unclaim` by the
claimer and by a maintainer, and for the mistakes the bot is meant to catch (an open pull request, an
`epic`, a status the field does not have, a missing token, a `/hold` written by someone who may not
hold, assignees newer than the event). It needs Node.js on the
PATH — the CI `docs` job runs it on every change — writes its two files under `target/claim-bot-test/`
and removes them again.
