# Herd — working agreement

Distributed build/test execution, "GitHub-runners shaped", project-agnostic.
A requester asks "build / test / check this commit"; Herd picks a machine that
fits and returns a result plus logs. First consumer: the chatOnS OS project.

## Roles

- **Alexa** (owner) decides *what* is built and *in which order*. Approves
  merges to `main`. Speaks French with Claude.
- **Claude** (orchestrator) designs, codes, tests, reports. Decides *how*,
  writes the decision down, and does not ask about it.
- Language: French in conversation. **English** in code, comments, commit
  messages, docs, issues.

## Vocabulary (use these words everywhere, no synonyms)

- **hub** — the service. One instance, WebSocket (`wss://`), runs in docker
  compose on the NixOS server.
- **runner** — a client that offers capacity and executes jobs. Dials out to
  the hub. The server runs one; the WSL PC runs one; more machines later.
- **requester** — whoever submits a job (AI agent, merger, nightly task),
  via `herdctl`.
- **job** — one unit of requested work with requirements, a priority, an exit
  code and logs.

## Invariants (do not break; changing one needs Alexa's approval)

1. **Stay specialized.** No workflow language, no plugin system, no cloud, no
   multi-tenant product surface. If a feature smells like generic CI, say no.
2. **A runner never destabilizes its host.** Every job runs under an explicit
   memory cap and time cap. A runner stops cleanly in **≤ 5 s** when its owner
   asks, and refuses new jobs while draining.
3. **Runners dial out.** No inbound port, no relay, no tunnel on client
   machines. The hub is the only thing that listens.
4. **The protocol is the contract.** All messages live in `herd-proto`, the
   handshake carries a protocol version, nothing is sent ad hoc.
5. **Done means a passing test.** "It compiles" is not done. Every milestone
   ships unit tests plus one real end-to-end hub+runner test.
6. **A requester never names a machine.** Selection is the hub's job, based on
   declared capabilities and priority.
7. **No secrets in the repo.** Config via environment variables or a file path
   given at startup.
8. **Docs discipline.** `docs/status.md` is *state*, replaced every session,
   never a journal. `docs/decisions.md` is append-only, one dated entry per
   decision. `docs/plan.md` holds the milestones.

## Layout

```
herd-proto/   message types, serde, protocol version      (library)
herd-hub/     the service: accepts runners, dispatches     (binary)
herd-runner/  the client: executes jobs, streams logs      (binary)
herdctl/      the requester CLI: submit, follow, cancel    (binary)
docs/         status.md, decisions.md, plan.md
```

## Commands

Toolchain comes from the Nix flake added in M1; nothing is installed globally.

```sh
nix develop                 # dev shell: rust stable, cargo, clippy, rustfmt
cargo build --workspace
cargo test --workspace      # unit tests + end-to-end test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p herd-hub       # local hub on 127.0.0.1, plain ws
cargo run -p herd-runner    # local runner pointing at it
```

Rule: `cargo test --workspace` and `cargo clippy` must be green before any
commit. No other suite is required per commit.

## Git

- One branch per milestone: `m1-end-to-end`, `m2-link-robustness`, …
- Small commits, imperative English subject, ≤ 72 chars, no scope prefixes
  required: `add job dispatch state machine`.
- Push the branch to GitHub as work progresses.
- **`main` only moves on Alexa's explicit approval.** Never merge or
  force-push `main` without it.

## When to ask Alexa

Ask only about things that change **what** gets built:

- a requirement that is ambiguous or conflicts with an invariant;
- the order of milestones, or dropping something from one;
- anything irreversible or outward-facing: merging to `main`, deploying to the
  server, opening/closing GitHub issues, touching NixOS config;
- a decision whose cost would be high to undo (new dependency that shapes the
  architecture, protocol break, storage choice).

Do not ask about: file layout, naming, crate internals, test style, library
choices that are easy to swap. Decide, note it in `docs/decisions.md` if it is
load-bearing, and move on.

One decision at a time, always with a recommendation.

## Working method

- Iterative: small increments that actually run.
- Claude works alone. A sub-agent only for one long, isolated task, never
  several in parallel, and never without Alexa's agreement.
- Read what is needed, not the whole repo. Short reports.
- chatOnS requests arrive as GitHub issues: triage them, propose where they
  fit in the plan, do not start them unprompted.

## End of every session

Update `docs/status.md` with: what works, what is not done and why, the next
action. Then report the same in three or four lines.
