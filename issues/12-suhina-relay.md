# `suhina-relay`: a binary the user runs, never infrastructure the project operates

**Type:** enhancement · **Area:** proposes **`collaboration`** (see Notes) · **Phase:** M8 or later — **the same milestone as issue 11, deliberately not a later one** · **Priority:** P2

A tenth to a third of connection pairs cannot connect directly: the large published measurement found hole punching succeeding on roughly **70 % (±7.1 %)** of attempts and about **21 % of measured peers with no public address at all**. So a relay is not an optimisation, it is the part that makes the promise true. [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) decides the constraint and the shape: the project operates no infrastructure, so the relay is a program the **user** runs, with a protocol documented so that a third party can write one — and the binary sits **beside `suhina-plugin-host`**, on that pattern, rather than in a repository of its own. What makes it "its own" is that somebody else is meant to run it, not where its source lives; a git repository has no legal meaning (ADR 0023 › Alternatives considered).

**Depends on:** issue 11 (the transport decision, which says how a relay is reached and what it must do) and the protocol it relays.

## Goal

A session that cannot connect directly still connects, through a relay that some participant chose to run — with no service the project hosts, no account, and nothing the project pays for over time.

## Scope

1. **The decision is already recorded, in two places, and they agree.** [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) decision 3 decides the project operates no infrastructure and that the relay is a separate binary the user runs with a documented protocol; its consequences place that binary **beside `suhina-plugin-host`**, on that pattern (a separate binary in the tree, with a documented protocol, health and restart — ADR 0006), and ADR 0004 §1 records the same reasoning for devices: engine-independent precisely so they *can* be moved out if that ever helps. **Do not create a second repository for this**: a repository is not a licence boundary (ADR 0023 › Alternatives), and the point of the relay is that it is easy to obtain, not that it is elsewhere. What must be separate is the **release** and the **documentation**, so a third party can run and reimplement it.
2. **The relay itself:** stateless, forwarding without being able to read the content — the shape the candidate stacks already use, where the operator sees connection metadata (identifiers, addresses, who paired with whom, byte counts) and **not** the audio or the project. Say plainly in the documentation what an operator can and cannot see, because a user who runs one is entitled to know.
3. **A documented protocol**, in `docs/spec/`, so a third party can implement a relay without reading our code. This is what makes "the user runs it" real rather than a slogan: if only our binary can do it, we are the infrastructure in every sense but the hosting bill.
4. **Being easy to run.** The whole point is that someone can run it at the moment it is needed. The bar: one command, no configuration file required, no container, and a clear statement of what it needs open. **Anything that makes it hard to obtain is a problem for this issue to solve, not to accept** — say in the issue how a user is expected to get it, whether that is a release artefact for their platform or a machine already in the session.
5. **Abuse.** A relay carrying strangers' traffic is an abuse target and the operator is a volunteer. Rate limiting, connection caps, an allow-list or a shared secret, and a way to see and stop what is happening. `SECURITY.md`'s scope applies to it, and the operator-not-project distinction must be explicit.
6. **Graceful absence.** A relay that is missing, unreachable or refusing connections is **an ordinary state**: the direct and hole-punched paths keep working, and the user is told what happened. This is the one component in the epic whose absence degrades by design rather than breaking a feature.
7. **Tests** (AGENTS.md rule 7): forwarding with the content unreadable, a cap being reached, a refusal, and a relay dying mid-session leaving the session recoverable.

## Acceptance criteria

- [ ] A relay binary exists, is built and released separately from the app, and **no shipped build depends on a relay the project operates**.
- [ ] `docs/spec/` documents the relay's protocol well enough for an independent implementation, and the issue records whether a second implementation was attempted.
- [ ] A session that cannot connect directly connects through a relay the user runs, on the network pairs issue 11 found failing.
- [ ] The relay forwards without being able to read the content, and what an operator can see is written down and matches the implementation.
- [ ] Rate limiting and connection caps exist and are configurable; reaching a cap refuses with a reason rather than dropping silently.
- [ ] A relay that is unreachable produces a clear message and leaves the direct paths working; a session in progress survives the relay disappearing, or reconnects and says so.
- [ ] The bar in Scope 4 is met: a user can be told in one line how to run it, and a test or a documented check shows the zero-configuration path works.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass; the relay's own tests run without a network where possible.
- [ ] Documents updated: `docs/spec/` (the protocol), `SECURITY.md` (the operator scope), `AGENTS.md` and `docs/architecture.md` if a crate or binary is added, `ROADMAP.md`, `docs/releasing.md` if it is released on its own schedule, and `ATTRIBUTION.md` for its dependencies.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **Anything the project hosts.** No service, no account, no managed relay, no default relay address in a shipped build.
- **The transport itself** (issue 11). This issue is what the transport falls back *to*.
- **Authentication of users**, which is the protocol's business (issue 10). A relay should be able to forward a session it cannot read.
- **Realtime audio** (issue 14) — though a relay carrying audio has the same shape and a much larger bandwidth bill, which is worth noting when the audio link is designed.
- Making the relay a Suhina **extension** (ADR 0015). A relay is optional infrastructure, not a capability of Suhina; the epic deliberately keeps collaboration out of the extension mechanism, and this is the piece where the distinction is easiest to blur.

## Notes

- **Area label.** None of the existing labels means "another machine"; **`collaboration` is proposed**.
- **Where it goes.** ADR 0023's consequences place the binary **beside `suhina-plugin-host`** — a separate binary **in this tree**, mirroring that precedent (a documented protocol, health and restart, ADR 0006) — with the relay's **protocol documented in `docs/spec/`** and its **release separate**, so a third party can run one and reimplement one. ADR 0004 §1 records the related reasoning for devices: engine-independent precisely so they *can* be moved to their own repository one day. That option stays open; it is not the starting position, and starting there would trade the thing that matters (easy to obtain) for the thing that does not (where the source lives).
- **The one tension this issue must not paper over:** the relay is needed *at the moment a connection fails*, which is the worst moment to send someone to a build page. Either the binary is easy enough to obtain that this is a short errand, or some participants keep one running — the issue should say which, and the ADR should decide whether a session can nominate a participant's machine as the relay (which needs no separate download at all for the machine that has a reachable address).
- **Related:** [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) (decision 3 and its consequences — where the binary goes), ADR 0006 (separate binary, documented protocol, health and restart), ADR 0004 §1 (engine-independent, movable later), ADR 0015 (why this is not an extension), [SECURITY.md](https://github.com/suhina-daw/suhina-daw/blob/main/SECURITY.md), issue 10, issue 11, issue 14, [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › Risks 1.
- **Decisions that belong in an ADR or spec, not here:** the relay's protocol; what it may log and what it must not; whether it authenticates anything; how a session is told which relay to use; whether a participant may be nominated as the relay for a session; and how the relay is versioned against the app.
- **A question to resolve first:** whether the relay is needed for a **first release at all**, given that issue 11 may choose to ship LAN and forwarded-port connections only. If it is not needed yet, this issue is still worth writing down now — the epic's risk is that the relay is deferred and the direct-only promise quietly becomes false — but its phase should say "the same milestone as the transport, whenever that is" rather than a date.
- **Board:** `enhancement`; Phase M8 or later, same milestone as issue 11; Priority P2.
