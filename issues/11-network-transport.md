# Network transport: a measured spike first, then direct, hole-punched and relayed connections

**Type:** enhancement · **Area:** proposes **`collaboration`** (see Notes) · **Phase:** M8 or later · **Priority:** P2

**The first deliverable of this issue is a measurement, not an implementation.** Which library is chosen, and whether hole punching works well enough on the connections our users actually have, is not a question that can be answered by reading documentation — [issue-146-stem-separation.md](https://github.com/suhina-daw/suhina-daw/blob/main/issue-146-stem-separation.md) settled its own runtime question the same way and said it plainly: *"the first step is a cheap spike, not a decision by reading."* This issue inherits that rule. The epic's constraint is harder than usual: **the project runs no infrastructure**, so the reachability story has to work with the user's own machines and nothing else.

**Depends on:** nothing to start the spike — **the spike is what the transport decision waits for.** The protocol it will carry is issue 10, and the relay it may fall back to is issue 12.

## What the spike must establish

1. **Which library.** The candidates are all licence-clean (rule 9 verified): `iroh` (MIT OR Apache-2.0), `quinn` (MIT OR Apache-2.0) with our own hole punching, `libp2p` (MIT), `mdns-sd` (Apache-2.0 OR MIT) for the local network. **The spike runs at least two of them on the same set of real networks and reports.** It must state what each pulls in — this would be the**first network code in the tree**, and ADR 0015 §3 records that there is no HTTP client today and that nothing is downloaded.
2. **What fraction of real pairs connect directly**, on: the same LAN; two home connections behind NAT; one behind carrier-grade NAT; one on a mobile connection; and two on the same machine. The published large-scale measurement found hole punching succeeding on **roughly 70 % (±7.1 %) of attempts**, and about **21 % of measured peers had no public address at all** — so the spike should expect a real share of failures and say how they present.
3. **How long a failure takes to be known**, because a user waiting thirty seconds to be told "it did not work" is a different product from one told in two.
4. **What the firewall asks for.** The published facts to verify: an outbound connection on the usual HTTPS port as the one strict requirement, and **outbound UDP to any destination port** for a direct connection — a narrowed UDP range does not work. That is a sentence that has to be true, or said differently, in user-facing text.
5. **Whether the invitation really removes the rendezvous server.** The design is that a host generates an invitation containing the addresses and a key, sends it out of band (a chat message, an e-mail), and **the invitation is the signalling channel** — so the first connection needs no third party at all. The spike proves or disproves this on a real pair.
6. **How long a connection survives idle**, since UDP mappings expire on the order of tens of seconds to a few minutes and a session paused for ten minutes must not silently die. Report what a keepalive costs.
7. **What happens when an address goes stale**, since an invitation embeds the addresses it was made with and a changed address invalidates it.

## Scope

1. **The spike**, as above, reported **in the issue** with the commands run and the numbers — not as a design document.
2. **The ADR**, `docs/decisions/00NN-network-transport.md`, written **after** the spike, deciding the library, the fallback order and the failure messages. With alternatives considered: no library (raw sockets and our own NAT traversal), a rendezvous service we run (rejected by the epic's constraint), and a hosted relay (rejected for the same reason).
3. **The reachability ladder, implemented in the order it is listed**, each rung a state the user can see rather than an internal hope:
   - **direct** — the host listens; LAN and IPv6 and a forwarded port need no third party at all;
   - **hole-punched** — both behind NAT, using the invitation as signalling;
   - **relayed** — a user-run relay (issue 12), never one the project operates.
4. **What the user is told**, which is part of the feature and not a message bolted on afterwards: no public address (carrier-grade NAT is common on mobile and some connections and cannot be fixed from our side), a firewall that is blocking, an invitation that has gone stale, and a direct connection that failed and is now relayed.
5. **Local network discovery** (mDNS) as the cheapest possible path: no addresses, no invitations, no third party on one network. It is worth more per line of code than anything else here.
6. **No new code reachable from the audio callback** (AGENTS.md rule 3), and **no network code below the UI** (`docs/architecture.md` › Layers).

## Acceptance criteria

- [ ] The spike is reported in the issue with the networks tested, the library versions, the direct/relayed split and the timings, including at least one **failure** case with how it presented.
- [ ] `docs/decisions/00NN-network-transport.md` exists with *Alternatives considered* (including "we run a rendezvous service", and why it is refused) and *Not decided yet*.
- [ ] Two instances on one local network find each other and connect with no invitation and no third party.
- [ ] Two instances on two networks connect from an invitation exchanged out of band, with no service the project operates — or the issue records exactly which network pairs this fails on and why.
- [ ] A host behind carrier-grade NAT is **detected** and told what it needs, rather than appearing to work and then timing out.
- [ ] A connection idle for the longest plausible pause survives, or reconnects, and the user is told which happened.
- [ ] A stale invitation fails with a message that says what to do, not a generic error.
- [ ] Nothing in the transport is reachable from `Renderer::process`, and no crate below the UI depends on a network library (checked, not assumed).
- [ ] Every new dependency has a row in `ATTRIBUTION.md` and is verified GPL-3.0-compatible (rule 9), including the TLS and cryptography the transport pulls in — `ring` is ISC with BoringSSL-derived parts and a permissively licensed `once_cell` polyfill, but the whole tail must be listed rather than assumed.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass, with the transport's own tests able to run offline.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **The protocol and the permissions** (issue 10). The transport carries bytes; it does not decide who may do what.
- **The relay binary** (issue 12). This issue decides that a relay is needed and how it is reached; the relay itself is separate, and separately releasable.
- **Realtime ensemble audio** (issue 14). This issue moves commands and state.
- **A discovery service, a matchmaking service, accounts, or anything hosted.** The epic refuses all of them, and the invitation is what replaces them.
- Using a third party's public relays in a shipped build.

## Notes

- **Area label.** As with issue 10, none of the existing area labels means "another machine"; **`collaboration` is proposed**.
- **Where the code goes** (`docs/architecture.md` › Where new code goes): a new layer-2/3 crate beside `automation`, with the transport behind a **trait** so a different one can be swapped — the pattern `crates/output` already uses for audio devices (`AudioSystem` is implemented by the output layer and handed to `Projects`; the session depends on the trait, not the implementation). **Nothing below the UI may depend on a network library.**
- **The numbers that decide the architecture, and the reason a relay is mandatory rather than nice:** roughly **70 % (±7.1 %)** direct in the large published measurement, **~21 %** of measured peers with no public address at all, and a vendor claim of about 90 % for a different stack. Either way a tenth to a third of pairs need a third party, so **issue 12 belongs in the same milestone as this issue** — never a later one.
- **Related:** ADR 0015 §2 and §3 (absence is an ordinary state; nothing is downloaded and no HTTP client is in the tree), ADR 0006 (the deadline-and-restart shape the protocol reuses), issue 10, issue 12, issue 14, [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › Risks 1 and 6, [issue-146-stem-separation.md](https://github.com/suhina-daw/suhina-daw/blob/main/issue-146-stem-separation.md) (the spike-first precedent).
- **Decisions that belong in the ADR, not here:** which library and which features; the fallback order and when each is tried; how long to wait before falling back; what is stored on disk between sessions, if anything; whether mDNS is on by default; and how the transport is versioned.
- **A question to resolve before the ADR:** whether the transport must work **without any relay at all** for a first release. If the answer is yes, the first release is LAN and mutually-forwarded-ports only, the reachability story is honest and small, and the relay becomes a follow-up — **that would make this issue much smaller and is worth considering.**
- **Board:** `enhancement`; Phase M8 or later; Priority P2. The spike alone is small and could be its own `good first issue` with a note that no network code may reach `main` until the ADR exists.
